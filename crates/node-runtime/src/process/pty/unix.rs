// Adapted from sailry-code 67ae9fa0, terminal-host/src/session.rs.
// See third_party_licenses/sailry-code-terminal.md.
use super::{Event, Output, failure};
use portable_pty::MasterPty;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    fs::File,
    io::{Read, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    sync::mpsc::{self, SyncSender, TryRecvError},
    thread::JoinHandle,
};

pub(crate) struct Io {
    input: SyncSender<Vec<u8>>,
    wake: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl Io {
    pub fn start(master: &dyn MasterPty, messages: Output) -> Result<Self, Fault> {
        let descriptor = master
            .as_raw_fd()
            .ok_or_else(|| failure("PTY descriptor is unavailable"))?;
        // SAFETY: master retains its descriptor during this call; dup returns an independent one.
        let duplicate = unsafe { libc::dup(descriptor) };
        if duplicate < 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        // SAFETY: dup succeeded and no owner has adopted or closed this new descriptor.
        // File owns it on every subsequent error path.
        let mut pty = unsafe { File::from_raw_fd(duplicate) };
        // SAFETY: File keeps duplicate live; F_GETFL takes no variadic argument.
        let flags = unsafe { libc::fcntl(duplicate, libc::F_GETFL) };
        // SAFETY: F_SETFL takes the integer flag mask for the still-owned descriptor.
        if flags < 0
            || unsafe { libc::fcntl(duplicate, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(failure(std::io::Error::last_os_error()));
        }
        let (wake, mut reader) = UnixStream::pair().map_err(failure)?;
        wake.set_nonblocking(true).map_err(failure)?;
        reader.set_nonblocking(true).map_err(failure)?;
        let (input, chunks) = mpsc::sync_channel::<Vec<u8>>(32);
        let thread = std::thread::Builder::new()
            .name("sailry-pty-io".into())
            .spawn(move || {
                let result = (|| -> std::io::Result<()> {
                    let mut buffer = [0u8; 16 * 1024];
                    let mut pending: Option<Vec<u8>> = None;
                    let mut offset = 0;
                    loop {
                        // Drain the next queued write after each partial write completes,
                        // including when wake bytes were coalesced while it was pending.
                        if pending.is_none() {
                            match chunks.try_recv() {
                                Ok(bytes) => pending = Some(bytes),
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => return Ok(()),
                            }
                        }
                        let mut poll = [
                            libc::pollfd {
                                fd: pty.as_raw_fd(),
                                events: libc::POLLIN
                                    | if pending.is_some() { libc::POLLOUT } else { 0 },
                                revents: 0,
                            },
                            libc::pollfd {
                                fd: reader.as_raw_fd(),
                                events: libc::POLLIN,
                                revents: 0,
                            },
                        ];
                        // SAFETY: poll is a writable array of exactly poll.len() entries;
                        // pty and reader retain both descriptors throughout the call.
                        if unsafe { libc::poll(poll.as_mut_ptr(), poll.len() as _, -1) } < 0 {
                            let error = std::io::Error::last_os_error();
                            if error.kind() == std::io::ErrorKind::Interrupted {
                                continue;
                            }
                            return Err(error);
                        }
                        if poll[1].revents != 0 {
                            if poll[1].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                                != 0
                            {
                                return Ok(());
                            }
                            let mut wake_bytes = [0; 128];
                            if matches!(reader.read(&mut wake_bytes), Ok(0)) {
                                return Ok(());
                            }
                        }
                        let hung_up = poll[0].revents & libc::POLLHUP != 0;
                        if poll[0].revents
                            & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                            != 0
                        {
                            match pty.read(&mut buffer) {
                                Ok(0) => return Ok(()),
                                Ok(count) => {
                                    if !messages(Event::Bytes(buffer[..count].to_vec())) {
                                        return Ok(());
                                    }
                                    if hung_up {
                                        continue;
                                    }
                                }
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::Interrupted
                                            | std::io::ErrorKind::WouldBlock
                                    ) =>
                                {
                                    if hung_up && error.kind() == std::io::ErrorKind::WouldBlock {
                                        return Ok(());
                                    }
                                }
                                Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                                    return Ok(());
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        if !hung_up
                            && poll[0].revents & libc::POLLOUT != 0
                            && let Some(bytes) = &pending
                        {
                            match pty.write(&bytes[offset..]) {
                                Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                                Ok(count) => {
                                    offset += count;
                                    if offset == bytes.len() {
                                        pending = None;
                                        offset = 0;
                                    }
                                }
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::Interrupted
                                            | std::io::ErrorKind::WouldBlock
                                    ) => {}
                                Err(error) => return Err(error),
                            }
                        }
                    }
                })();
                messages(Event::Eof(result.map_err(|error| error.to_string())));
            })
            .map_err(failure)?;
        Ok(Self {
            input,
            wake,
            thread: Some(thread),
        })
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<(), Fault> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.input.try_send(bytes).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => {
                Fault::new(ErrorCode::Busy, "terminal input queue is full")
            }
            mpsc::TrySendError::Disconnected(_) => {
                Fault::new(ErrorCode::Unavailable, "terminal input is closed")
            }
        })?;
        match (&self.wake).write(&[1]) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                format!("terminal input wake failed: {error}"),
            )),
        }
    }

    pub fn stop(&mut self) {
        let _ = self.wake.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Io {
    fn drop(&mut self) {
        self.stop();
    }
}
