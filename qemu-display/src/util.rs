use crate::Result;

#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(windows)]
use win32::Fd;
#[cfg(unix)]
use zbus::zvariant::Fd;

#[cfg(windows)]
use crate::win32;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;
#[cfg(windows)]
use uds_windows::UnixStream;
#[cfg(windows)]
use windows::Win32::Networking::WinSock::SOCKET;
#[cfg(windows)]
use windows::Win32::System::Threading::PROCESS_DUP_HANDLE;

/// Makes the socketpair a listener runs over, in place of `UnixStream::pair()`.
///
/// QEMU authenticates a listener with the credentials of its end, which are those of the process
/// that made the pair. A client in another pid namespace than QEMU's shows up there as pid 0,
/// which GDBus rejects; such a client has a process in QEMU's namespace make the pair instead.
pub type SocketPairFn = std::sync::Arc<
    dyn Fn() -> futures::future::BoxFuture<'static, std::io::Result<(UnixStream, UnixStream)>>
        + Send
        + Sync,
>;

pub(crate) async fn socket_pair(
    make: Option<&SocketPairFn>,
) -> std::io::Result<(UnixStream, UnixStream)> {
    match make {
        Some(make) => make().await,
        None => UnixStream::pair(),
    }
}

pub fn prepare_uds_pass(#[cfg(windows)] peer_pid: u32, us: &UnixStream) -> Result<Fd<'_>> {
    #[cfg(unix)]
    {
        Ok(us.into())
    }

    #[cfg(windows)]
    {
        let p = win32::ProcessHandle::open(Some(peer_pid as _), PROCESS_DUP_HANDLE)?;
        p.duplicate_socket(SOCKET(us.as_raw_socket() as _))
    }
}
