//! One owned WebDriver process tree; never attach to the user's personal browser.
use super::*;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::process::Stdio;

pub(super) struct Driver {
    child: Box<dyn ChildWrapper>,
    armed: bool,
}

impl Drop for Driver {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.child.start_kill();
        }
    }
}

impl Driver {
    pub async fn start(
        bundle: &bundle::Bundle,
        stop: &CancellationToken,
    ) -> Result<(Self, String), Fault> {
        let listener =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(io_error)?;
        let port = listener.local_addr().map_err(io_error)?.port();
        let mut command = tokio::process::Command::new(&bundle.driver);
        command
            .args([format!("--port={port}"), "--allowed-ips=127.0.0.1".into()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut command = CommandWrap::from(command);
        command.wrap(KillOnDrop);
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        drop(listener);
        let mut process = Self {
            child: command.spawn().map_err(io_error)?,
            armed: true,
        };
        let url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .map_err(|_| unavailable("browser driver client unavailable"))?;
        let ready = async {
            for _ in 0..100 {
                if process.child.try_wait().map_err(io_error)?.is_some() {
                    return Err(unavailable("browser driver exited before becoming ready"));
                }
                if let Ok(response) = client.get(format!("{url}/status")).send().await
                    && response.status().is_success()
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(unavailable("browser driver did not become ready"))
        };
        tokio::select! {
            _ = stop.cancelled() => return Err(unavailable("browser driver startup cancelled")),
            result = ready => result?,
        }
        Ok((process, url))
    }

    pub async fn close(mut self) -> Result<(), Fault> {
        #[cfg(unix)]
        let group = self.child.id();
        // SAFETY: ProcessGroup/JobObject retains ownership of this exact native child.
        // Reap an exited root before signalling its group, as in the shared MCP adapter.
        unsafe { self.child.try_inner_child_mut() }
            .expect("driver owns a native process")
            .try_wait()
            .map_err(io_error)?;
        let killed = self.child.start_kill();
        // SAFETY: the wrapper continues to own the process tree while Tokio reaps its root.
        tokio::time::timeout(
            Duration::from_secs(5),
            unsafe { self.child.try_inner_child_mut() }
                .expect("driver owns a native process")
                .wait(),
        )
        .await
        .map_err(|_| unavailable("browser driver shutdown timed out"))?
        .map_err(io_error)?;
        if let Err(error) = killed {
            #[cfg(unix)]
            if error.raw_os_error() == Some(libc::ESRCH)
                || (error.raw_os_error() == Some(libc::EPERM)
                    && group.is_some_and(|group| {
                        // A non-signalling probe distinguishes an absent group from denied cleanup.
                        (unsafe { libc::kill(-(group as i32), 0) }) == -1
                            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                    }))
            {
                self.armed = false;
                return Ok(());
            }
            return Err(io_error(error));
        }
        self.armed = false;
        Ok(())
    }
}
