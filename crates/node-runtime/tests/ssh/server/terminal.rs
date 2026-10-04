//! The SSH fixture runs a real PTY so input, window changes and exit are observable.
use super::*;
use portable_pty::{CommandBuilder, MasterPty, PtySize, SlavePty};
use std::io::{Read, Write};

pub(super) struct Pty {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    slave: Option<Box<dyn SlavePty + Send>>,
}

fn size(columns: u32, rows: u32, width: u32, height: u32) -> PtySize {
    PtySize {
        cols: columns as u16,
        rows: rows as u16,
        pixel_width: width as u16,
        pixel_height: height as u16,
    }
}

impl Pty {
    pub fn new(columns: u32, rows: u32, width: u32, height: u32) -> Self {
        let pair = portable_pty::native_pty_system()
            .openpty(size(columns, rows, width, height))
            .unwrap();
        Self {
            master: Arc::new(Mutex::new(pair.master)),
            slave: Some(pair.slave),
        }
    }

    pub fn resize(&self, columns: u32, rows: u32, width: u32, height: u32) {
        self.master
            .lock()
            .unwrap()
            .resize(size(columns, rows, width, height))
            .unwrap();
    }

    pub fn start(
        &mut self,
        root: PathBuf,
        mut channel: Channel<server::Msg>,
        handle: server::Handle,
    ) -> tokio::task::JoinHandle<()> {
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-i");
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        command.env("PS1", "fixture> ");
        let slave = self.slave.take().unwrap();
        let mut child = slave.spawn_command(command).unwrap();
        drop(slave);
        let master = self.master.clone();
        let mut writer = master.lock().unwrap().take_writer().unwrap();
        let mut reader = master.lock().unwrap().try_clone_reader().unwrap();
        let id = channel.id();
        let runtime = tokio::runtime::Handle::current();
        tokio::spawn(async move {
            let output = handle.clone();
            let reader = tokio::task::spawn_blocking(move || {
                let mut buffer = [0; 16384];
                while let Ok(count) = reader.read(&mut buffer) {
                    if count == 0
                        || runtime
                            .block_on(output.data(id, buffer[..count].to_vec()))
                            .is_err()
                    {
                        break;
                    }
                }
            });
            let mut poll = tokio::time::interval(Duration::from_millis(20));
            let code = loop {
                tokio::select! {
                    _ = poll.tick() => {
                        if let Some(status) = child.try_wait().unwrap() { break status.exit_code(); }
                    }
                    message = channel.wait() => match message {
                        Some(russh::ChannelMsg::Data { data }) => {
                            let (returned, result) = tokio::task::spawn_blocking(move || {
                                let result = writer.write_all(&data);
                                (writer, result)
                            }).await.unwrap();
                            writer = returned;
                            if result.is_err() { break 255; }
                        }
                        Some(russh::ChannelMsg::Close) | None => break 255,
                        _ => {}
                    }
                }
            };
            let _ = child.kill();
            drop(writer);
            drop(master);
            tokio::task::spawn_blocking(move || child.wait())
                .await
                .unwrap()
                .unwrap();
            reader.await.unwrap();
            let _ = handle.exit_status_request(id, code).await;
            let _ = handle.eof(id).await;
            let _ = handle.close(id).await;
        })
    }
}
