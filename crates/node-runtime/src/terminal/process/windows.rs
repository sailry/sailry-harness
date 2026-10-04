use super::{Message, failure};
use portable_pty::MasterPty;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};
use windows_sys::Win32::System::IO::CancelSynchronousIo;

pub(super) struct Io {
    input: Option<SyncSender<Vec<u8>>>,
    stopped: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}

impl Io {
    pub fn start(master: &dyn MasterPty, messages: SyncSender<Message>) -> Result<Self, Fault> {
        let mut reader = master.try_clone_reader().map_err(failure)?;
        let mut writer = master.take_writer().map_err(failure)?;
        let (input, chunks) = mpsc::sync_channel::<Vec<u8>>(32);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let output = messages.clone();
        let read = std::thread::Builder::new()
            .name("sailry-pty-read".into())
            .spawn(move || {
                let mut buffer = [0u8; 16 * 1024];
                let result = loop {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    match reader.read(&mut buffer) {
                        Ok(0) => break Ok(()),
                        Ok(count) => {
                            if output
                                .send(Message::Bytes(buffer[..count].to_vec()))
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                            break Ok(());
                        }
                        Err(error) => break Err(error.to_string()),
                    }
                };
                let _ = output.send(Message::Eof(result));
            })
            .map_err(failure)?;
        let stop = stopped.clone();
        let write = std::thread::Builder::new()
            .name("sailry-pty-write".into())
            .spawn(move || {
                while let Ok(bytes) = chunks.recv() {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    if let Err(error) = writer.write_all(&bytes) {
                        let _ = messages.send(Message::Eof(Err(error.to_string())));
                        return;
                    }
                }
            });
        let mut io = Self {
            input: Some(input),
            stopped,
            reader: Some(read),
            writer: None,
        };
        match write {
            Ok(thread) => io.writer = Some(thread),
            Err(error) => {
                io.stop();
                return Err(failure(error));
            }
        }
        Ok(io)
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<(), Fault> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.input
            .as_ref()
            .ok_or_else(|| failure("terminal input is closed"))?
            .try_send(bytes)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    Fault::new(ErrorCode::Busy, "terminal input queue is full")
                }
                mpsc::TrySendError::Disconnected(_) => {
                    Fault::new(ErrorCode::Unavailable, "terminal input is closed")
                }
            })
    }

    pub fn stop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.input.take();
        for thread in [&mut self.writer, &mut self.reader] {
            if let Some(thread) = thread.take() {
                // Retain the exact thread handle. Reissue cancellation to cover the
                // check-to-read race; never terminate the thread or search by PID.
                while !thread.is_finished() {
                    unsafe { CancelSynchronousIo(thread.as_raw_handle()) };
                    std::thread::sleep(Duration::from_millis(5));
                }
                let _ = thread.join();
            }
        }
    }
}

impl Drop for Io {
    fn drop(&mut self) {
        self.stop();
    }
}
