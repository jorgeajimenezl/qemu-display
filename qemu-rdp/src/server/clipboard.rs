use std::sync::{Arc, Mutex};

use anyhow::Result;
use ironrdp::{
    cliprdr::{
        backend::{ClipboardMessage, CliprdrBackend, CliprdrBackendFactory},
        pdu::{
            ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags,
            FileContentsRequest, FileContentsResponse, FormatDataRequest, FormatDataResponse,
            LockDataId, OwnedFormatDataResponse,
        },
    },
    core::{impl_as_any, ReadCursor},
    pdu::utils::{read_string_from_cursor, CharacterSet},
    server::{CliprdrServerFactory, ServerEvent, ServerEventSender},
};
use tracing::{debug, error, warn};

use qemu_display::{Clipboard, ClipboardSelection, Display};
use tokio::{
    sync::{
        mpsc::{self, Receiver, Sender},
        oneshot,
    },
    task,
};

#[derive(Debug)]
pub struct Inner {
    registered: bool,
    clipboard: Clipboard,
    tx: Sender<ClipboardEvent>,
    selection: ClipboardSelection,
    serial: u32,
    ev_sender: Option<mpsc::UnboundedSender<ServerEvent>>,
    dbus_request: Option<(oneshot::Sender<Vec<u8>>, ClipboardFormatId)>,
    _task: Option<task::JoinHandle<()>>,
}

#[derive(Clone, Debug)]
pub struct ClipboardHandler {
    inner: Arc<Mutex<Inner>>,
}

#[async_trait::async_trait]
impl qemu_display::ClipboardHandler for ClipboardHandler {
    async fn register(&mut self) {
        let mut inner = self.inner.lock().unwrap();

        inner.serial = 0;
        inner.dbus_request = None;
    }

    async fn unregister(&mut self) {}

    async fn grab(&mut self, selection: ClipboardSelection, serial: u32, mimes: Vec<String>) {
        debug!(?selection, ?serial, "Grab -> RDP");
        let mut inner = self.inner.lock().unwrap();

        if selection != inner.selection {
            return;
        }

        if serial < inner.serial {
            warn!(?serial, ?inner.serial, "discarding");
            return;
        }

        inner.serial = serial;

        let format_list = mimes
            .iter()
            .filter_map(|f| match f.as_str() {
                "text/plain;charset=utf-8" => {
                    let cf = ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT);
                    Some(cf)
                }
                _ => None,
            })
            .collect();

        if let Some(svc_sender) = inner.ev_sender.as_mut() {
            if let Err(e) = svc_sender.send(ServerEvent::Clipboard(
                ClipboardMessage::SendInitiateCopy(format_list),
            )) {
                error!(?e, "failed to send SVC message");
            }
        }
    }

    async fn release(&mut self, selection: ClipboardSelection) {
        debug!(?selection, "Release -> RDP");
        let mut inner = self.inner.lock().unwrap();

        if selection != inner.selection {
            return;
        }

        if let Some(svc_sender) = inner.ev_sender.as_mut() {
            if let Err(e) = svc_sender.send(ServerEvent::Clipboard(
                ClipboardMessage::SendInitiateCopy(vec![]),
            )) {
                error!(?e, "failed to send SVC message");
            }
        }
    }

    async fn request(
        &mut self,
        selection: ClipboardSelection,
        mimes: Vec<String>,
    ) -> qemu_display::Result<(String, Vec<u8>)> {
        debug!(?selection, ?mimes, "Request -> RDP");

        let (rx, format) = {
            let mut inner = self.inner.lock().unwrap();

            if selection != inner.selection {
                return Err(qemu_display::Error::Failed(
                    "Unhandled clipboard selection".into(),
                ));
            }

            // since we are blocking on Request method handling, we shouldn't get there again
            if inner.dbus_request.is_some() {
                return Err(qemu_display::Error::Failed(
                    "Pending clipboard request!".into(),
                ));
            }

            let format = {
                let mut format_found = None;
                for mime in mimes.iter() {
                    match mime.as_str() {
                        "text/plain;charset=utf-8" => {
                            format_found = Some(ClipboardFormatId::CF_UNICODETEXT);
                            break;
                        }
                        _ => continue,
                    }
                }
                format_found
            };

            let Some(format) = format else {
                return Err(qemu_display::Error::Failed("Unhandled MIMEs".into()));
            };

            let (tx, rx) = oneshot::channel();
            inner.dbus_request = Some((tx, format));
            if let Some(svc_sender) = inner.ev_sender.as_mut() {
                if let Err(e) = svc_sender.send(ServerEvent::Clipboard(
                    ClipboardMessage::SendInitiatePaste(format),
                )) {
                    error!(?e, "failed to send SVC message");
                }
            }

            (rx, format)
        };

        let data = rx
            .await
            .map_err(|_| qemu_display::Error::Failed("Failed to get clipboard data".into()))?;

        let data = match format {
            ClipboardFormatId::CF_UNICODETEXT => {
                let mut cursor = ReadCursor::new(&data);
                let s = read_string_from_cursor(&mut cursor, CharacterSet::Unicode, true)
                    .map_err(|_| qemu_display::Error::Failed("Failed to convert string".into()))?;
                s.into_bytes()
            }
            _ => unimplemented!(),
        };

        Ok(("text/plain;charset=utf-8".into(), data))
    }
}

#[derive(Debug)]
enum ClipboardEvent {
    Register,
    Grab {
        selection: ClipboardSelection,
        serial: u32,
        available_formats: Vec<ClipboardFormat>,
    },
    Release {
        selection: ClipboardSelection,
    },
    Request {
        selection: ClipboardSelection,
        request: FormatDataRequest,
    },
}

async fn rdp_clipboard_receive_task(mut rx: Receiver<ClipboardEvent>, cb: ClipboardHandler) {
    let clipboard = {
        let inner = cb.inner.lock().unwrap();
        inner.clipboard.clone()
    };

    loop {
        let res = match rx.recv().await {
            Some(ClipboardEvent::Register) => {
                debug!("Register -> dbus");

                clipboard.register(cb.clone()).await
            }
            Some(ClipboardEvent::Grab {
                selection,
                serial,
                available_formats,
            }) => {
                let mimes = available_formats
                    .iter()
                    .filter_map(|f| match f.id {
                        ClipboardFormatId::CF_UNICODETEXT => {
                            Some("text/plain;charset=utf-8".to_string())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();

                debug!(?mimes, "Grab -> dbus");

                let mimes: Vec<&str> = mimes.iter().map(AsRef::as_ref).collect();

                clipboard
                    .proxy
                    .grab(selection, serial, &mimes)
                    .await
                    .map_err(Into::into)
            }
            Some(ClipboardEvent::Release { selection }) => {
                debug!("Release -> dbus");

                clipboard.proxy.release(selection).await.map_err(Into::into)
            }
            Some(ClipboardEvent::Request { selection, request }) => {
                debug!(?request, "Request -> dbus");

                let mime = match request.format {
                    ClipboardFormatId::CF_UNICODETEXT => "text/plain;charset=utf-8",
                    fmt => {
                        debug!(?fmt, "unhandled requested format");
                        continue;
                    }
                };

                let res = clipboard.proxy.request(selection, &[mime]).await;
                if let Ok(res) = res {
                    let data = match (request.format, res.0.as_str()) {
                        (ClipboardFormatId::CF_UNICODETEXT, "text/plain;charset=utf-8") => {
                            let Ok(s) = std::str::from_utf8(&res.1) else {
                                error!("Invalid text data");
                                continue;
                            };
                            OwnedFormatDataResponse::new_unicode_string(s)
                        }
                        (_, mime) => {
                            debug!(?mime, "Unsupported data format");
                            continue;
                        }
                    };
                    let mut inner = cb.inner.lock().unwrap();
                    if let Some(svc_sender) = inner.ev_sender.as_mut() {
                        if let Err(e) = svc_sender.send(ServerEvent::Clipboard(
                            ClipboardMessage::SendFormatData(data),
                        )) {
                            error!(?e, "failed to send SVC message");
                        }
                    }
                } else {
                    warn!(?res, "Request dbus reply");
                }
                Ok(())
            }
            None => break,
        };

        if let Err(e) = res {
            error!(?e, "clipboard task handling error");
        }
    }
}

impl Inner {
    fn register(&mut self) {
        if self.registered {
            return;
        }
        if let Err(e) = self.tx.try_send(ClipboardEvent::Register) {
            error!(?e, "clipboard register error");
        }
        self.registered = true;
    }

    fn grab(&mut self, available_formats: Vec<ClipboardFormat>) {
        if let Err(e) = self.tx.try_send(ClipboardEvent::Grab {
            selection: self.selection,
            serial: self.serial,
            available_formats,
        }) {
            error!(?e, "clipboard grab error");
        } else {
            self.serial += 1;
        }
    }

    fn release(&mut self) {
        if let Err(e) = self.tx.try_send(ClipboardEvent::Release {
            selection: self.selection,
        }) {
            error!(?e, "clipboard release error");
        }
    }

    fn request(&mut self, request: FormatDataRequest) {
        if let Err(e) = self.tx.try_send(ClipboardEvent::Request {
            selection: self.selection,
            request,
        }) {
            error!(?e, "clipboard request error");
        }
    }

    fn data(&mut self, data: Vec<u8>) {
        debug!("Data -> dbus");

        if let Some((data_tx, _fmt)) = self.dbus_request.take() {
            if let Err(e) = data_tx.send(data) {
                error!(?e, "failed to send clipboard data to dbus");
            }
        }
    }
}

impl ClipboardHandler {
    pub async fn connect(display: &Display<'_>) -> Result<Self> {
        let clipboard = Clipboard::new(display.connection()).await?;
        let selection = ClipboardSelection::Clipboard;
        let (tx, rx) = tokio::sync::mpsc::channel(30);

        let inner = Arc::new(Mutex::new(Inner {
            tx,
            selection,
            serial: 0,
            clipboard,
            ev_sender: None,
            dbus_request: None,
            _task: None,
            registered: false,
        }));

        let s = Self { inner };
        let clone = s.clone();
        let _task = task::spawn(async move { rdp_clipboard_receive_task(rx, clone).await });
        s.inner.lock().unwrap()._task = Some(_task);

        Ok(s)
    }
}

// impl Server
impl ServerEventSender for ClipboardHandler {
    fn set_sender(&mut self, sender: mpsc::UnboundedSender<ServerEvent>) {
        let mut inner = self.inner.lock().unwrap();

        inner.ev_sender = Some(sender);
    }
}

impl CliprdrServerFactory for ClipboardHandler {}

#[derive(Debug)]
pub(crate) struct RDPCliprdrBackend {
    inner: Arc<Mutex<Inner>>,
}

impl_as_any!(RDPCliprdrBackend);

impl CliprdrBackendFactory for ClipboardHandler {
    fn build_cliprdr_backend(&self) -> Box<dyn CliprdrBackend> {
        Box::new(RDPCliprdrBackend {
            inner: self.inner.clone(),
        })
    }
}

impl CliprdrBackend for RDPCliprdrBackend {
    fn temporary_directory(&self) -> &str {
        ".cliprdr"
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        self.inner.lock().unwrap().register();

        // No additional capabilities yet
        ClipboardGeneralCapabilityFlags::empty()
    }

    fn on_ready(&mut self) {
        // todo
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        capabilities: ClipboardGeneralCapabilityFlags,
    ) {
        debug!(?capabilities);
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        debug!(?available_formats);

        let mut inner = self.inner.lock().unwrap();
        if available_formats.is_empty() {
            inner.release();
        } else {
            inner.grab(available_formats.into());
        }
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        debug!(?request);

        self.inner.lock().unwrap().request(request);
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        debug!(?response);

        self.inner.lock().unwrap().data(response.into_data().into());
    }

    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        debug!(?request);
    }

    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        debug!(?response);
    }

    fn on_lock(&mut self, data_id: LockDataId) {
        debug!(?data_id);
    }

    fn on_unlock(&mut self, data_id: LockDataId) {
        debug!(?data_id);
    }

    fn on_request_format_list(&mut self) {
        debug!("on_request_format_list");
    }
}
