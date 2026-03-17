use async_broadcast::{broadcast, Receiver, Sender};
use async_lock::RwLock;
use futures::Stream;
use std::{
    collections::HashMap,
    fmt::Debug,
    os::unix::net::UnixStream,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::{Chardev, Error, Result};
#[cfg(windows)]
use uds_windows::UnixStream;

/// Backend trait for USB redirection implementations.
///
/// Implementations provide device-side protocol handling. The channel manager
/// handles QEMU D-Bus chardev selection and socket pair creation.
#[async_trait::async_trait]
pub trait UsbRedirBackend: Send + Sync {
    type Device: Send + Sync + Debug;
    type Key: Debug + Send + Sync + Eq + std::hash::Hash + for<'a> From<&'a Self::Device>;
    type Session: Send + Debug;

    /// Starts a redirection session for the given device using the provided unix stream.
    async fn start_session(
        &self,
        device: &Self::Device,
        stream: UnixStream,
    ) -> Result<Self::Session>;
}

#[derive(Debug, Clone, Copy)]
enum Event {
    NFreeChannels(i32),
}

#[derive(Debug)]
struct Inner<B: UsbRedirBackend> {
    chardevs: Vec<Chardev>,
    sessions: HashMap<B::Key, B::Session>,
    channel: (Sender<Event>, Receiver<Event>),
    backend: B,
}

impl<B: UsbRedirBackend> Inner<B> {
    // could make use of async combinators..
    async fn first_available_chardev(&self) -> Option<&Chardev> {
        for c in &self.chardevs {
            if c.proxy.owner().await.unwrap_or_default().is_empty() {
                return Some(c);
            }
        }
        None
    }

    async fn n_available_chardev(&self) -> usize {
        let mut n = 0;
        for c in &self.chardevs {
            if c.proxy.owner().await.unwrap_or_default().is_empty() {
                n += 1;
            }
        }
        n
    }
}

#[derive(Debug, Clone)]
pub struct UsbRedir<B: UsbRedirBackend> {
    inner: Arc<RwLock<Inner<B>>>,
}

impl<B: UsbRedirBackend> UsbRedir<B> {
    pub fn new(chardevs: Vec<Chardev>, backend: B) -> Self {
        let mut channel = broadcast(1);
        channel.0.set_overflow(true);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                chardevs,
                channel,
                sessions: Default::default(),
                backend,
            })),
        }
    }

    pub async fn set_device_state(&self, device: &B::Device, state: bool) -> Result<bool> {
        let mut inner = self.inner.write().await;
        let key = B::Key::from(device);
        let handled = inner.sessions.contains_key(&key);
        // We should do better and watch for owner properties changes, but this would require tasks
        // anticipate result
        let mut nfree = inner.n_available_chardev().await as _;

        match (state, handled) {
            (true, false) => {
                let chardev = inner
                    .first_available_chardev()
                    .await
                    .ok_or_else(|| Error::Failed("There are no free USB channels".into()))?;

                let (stream, peer) = UnixStream::pair()?;
                let session = inner.backend.start_session(device, stream).await?;
                chardev.proxy.register((&peer).into()).await?;
                inner.sessions.insert(key, session);
                nfree -= 1;
            }
            (false, true) => {
                inner.sessions.remove(&key);
                nfree += 1;
            }
            _ => {
                return Ok(state);
            }
        }

        let _ = inner.channel.0.broadcast(Event::NFreeChannels(nfree)).await;

        Ok(state)
    }

    pub async fn is_device_connected(&self, device: &B::Device) -> bool {
        let inner = self.inner.read().await;

        inner.sessions.contains_key(&B::Key::from(device))
    }

    pub async fn n_free_channels(&self) -> i32 {
        let inner = self.inner.read().await;

        inner.n_available_chardev().await as _
    }

    pub async fn receive_n_free_channels(&self) -> Pin<Box<dyn Stream<Item = i32>>> {
        let inner = self.inner.read().await;

        Box::pin(NFreeChannelsStream {
            receiver: inner.channel.1.clone(),
        })
    }
}

#[derive(Debug)]
struct NFreeChannelsStream {
    receiver: Receiver<Event>,
}

impl Stream for NFreeChannelsStream {
    type Item = i32;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = Pin::new(self);

        match Stream::poll_next(Pin::new(&mut this.receiver), cx) {
            Poll::Ready(Some(Event::NFreeChannels(n))) => Poll::Ready(Some(n)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
