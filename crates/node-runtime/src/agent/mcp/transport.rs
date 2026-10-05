//! ADK owns tool adaptation; Node retains transport closure and child reaping.
//! McpToolset does not expose RunningService::close, so both initialization failure
//! and normal completion observe actual transport cleanup, not is_closed().
use super::*;
use crate::plugins::resources::mcp::Launch;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use rmcp::{
    RoleClient,
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use std::{io, process::Stdio};
use tokio::sync::oneshot;
mod sse;

struct Tracked<T: Transport<RoleClient> + 'static> {
    inner: Option<T>,
    closed: Option<oneshot::Sender<io::Result<()>>>,
}

impl<T: Transport<RoleClient> + 'static> Drop for Tracked<T> {
    fn drop(&mut self) {
        if let (Some(mut transport), Some(closed)) = (self.inner.take(), self.closed.take()) {
            // The SDK drops its transport on failed/cancelled initialization without
            // calling close. Node retains and awaits this completion before reaping.
            tokio::spawn(async move {
                let result = transport.close().await.map_err(|_| failure());
                drop(transport);
                let _ = closed.send(result);
            });
        }
    }
}

impl<T: Transport<RoleClient> + 'static> Transport<RoleClient> for Tracked<T> {
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let send = self
            .inner
            .as_mut()
            .expect("MCP transport is retained")
            .send(item);
        async move { send.await.map_err(|_| failure()) }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        self.inner.as_mut()?.receive().await
    }
    async fn close(&mut self) -> io::Result<()> {
        let Some(mut transport) = self.inner.take() else {
            return Ok(());
        };
        let result = transport.close().await;
        drop(transport);
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(if result.is_ok() {
                Ok(())
            } else {
                Err(failure())
            });
        }
        result.map_err(|_| failure())
    }
}

fn failure() -> io::Error {
    io::Error::other("MCP transport failed")
}

struct Child {
    inner: Box<dyn ChildWrapper>,
    _roots: (cap_std::fs::Dir, cap_std::fs::Dir),
    settings: Option<crate::plugins::resources::mcp::Projection>,
    armed: bool,
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.inner.start_kill();
        }
    }
}

impl Child {
    async fn close(&mut self) -> io::Result<()> {
        let process = self.close_process().await;
        let settings = if let Some(file) = self.settings.take() {
            tokio::task::spawn_blocking(move || file.close())
                .await
                .map_err(|_| failure())?
        } else {
            Ok(())
        };
        process.and(settings)
    }

    async fn close_process(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        let group = self.inner.id();
        // Reap an already-exited root first. macOS may reject killpg on a group
        // containing only that zombie even though there is nothing left to kill.
        // SAFETY: the wrapper retains this exact native child and group ownership.
        unsafe { self.inner.try_inner_child_mut() }
            .expect("MCP owns a native process")
            .try_wait()?;
        let killed = self.inner.start_kill();
        self.armed = false;
        // Same retained native-root wait as the existing command process owner.
        // SAFETY: only ProcessGroup/JobObject owns this native child. Group cleanup
        // remains with the wrapper; Tokio waits and reaps the exact root process.
        unsafe { self.inner.try_inner_child_mut() }
            .expect("MCP owns a native process")
            .wait()
            .await?;
        if let Err(error) = killed {
            #[cfg(unix)]
            {
                if error.raw_os_error() == Some(libc::ESRCH) {
                    return Ok(());
                }
                // The root can exit between try_wait and killpg. After reaping it,
                // accept EPERM only when a non-signalling probe proves the exact
                // retained group is absent; a live/unowned group remains an error.
                if error.raw_os_error() == Some(libc::EPERM)
                    && group.is_some_and(|group| {
                        (unsafe { libc::kill(-(group as i32), 0) }) == -1
                            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                    })
                {
                    return Ok(());
                }
            }
            return Err(error);
        }
        Ok(())
    }
}

pub(super) struct Connection {
    stop: CancellationToken,
    closed: Option<oneshot::Receiver<io::Result<()>>>,
    child: Option<Child>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Connection {
    pub(super) async fn open(
        launch: Launch,
        authorized: Option<rmcp::transport::auth::AuthClient<reqwest::Client>>,
        turn_stop: &CancellationToken,
        handler: Arc<input::Handler>,
    ) -> Result<(Self, Service), Fault> {
        match launch {
            Launch::Stdio {
                command,
                args,
                env,
                cwd,
                roots,
                settings,
            } => {
                let mut command = tokio::process::Command::new(command);
                crate::process::Environment::capture().apply(&mut command);
                command
                    .args(args)
                    .envs(env)
                    .current_dir(cwd)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                let mut command = CommandWrap::from(command);
                command.wrap(KillOnDrop);
                #[cfg(unix)]
                command.wrap(process_wrap::tokio::ProcessGroup::leader());
                #[cfg(windows)]
                command.wrap(process_wrap::tokio::JobObject);
                if turn_stop.is_cancelled() {
                    return Err(unavailable());
                }
                let mut child = Child {
                    inner: command.spawn().map_err(|_| unavailable())?,
                    _roots: roots,
                    settings,
                    armed: true,
                };
                let input = child.inner.stdin().take().expect("MCP stdin was piped");
                let output = child.inner.stdout().take().expect("MCP stdout was piped");
                Self::connect(
                    rmcp::transport::async_rw::AsyncRwTransport::new_client(
                        limits::Lines::new(output),
                        input,
                    ),
                    Some(child),
                    turn_stop,
                    handler,
                )
                .await
            }
            Launch::Http {
                url, headers, sse, ..
            } => {
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(10))
                    .build()
                    .map_err(|_| unavailable())?;
                // Protocol-controlled fields take precedence; bound credentials stay private.
                let headers = headers
                    .into_iter()
                    .filter(|(name, _)| !crate::plugins::mcp::protocol_header(name))
                    .filter(|(name, _)| {
                        authorized.is_none() || !name.eq_ignore_ascii_case("authorization")
                    })
                    .map(|(name, value)| {
                        let mut value = reqwest::header::HeaderValue::from_str(&value)
                            .map_err(|_| unavailable())?;
                        value.set_sensitive(true);
                        Ok((
                            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                                .map_err(|_| unavailable())?,
                            value,
                        ))
                    })
                    .collect::<Result<_, Fault>>()?;
                if let Some(client) = authorized {
                    Self::http(client, url, headers, sse, turn_stop, handler).await
                } else {
                    Self::http(client, url, headers, sse, turn_stop, handler).await
                }
            }
        }
    }

    async fn http<C: rmcp::transport::streamable_http_client::StreamableHttpClient + Sync>(
        client: C,
        url: String,
        headers: std::collections::HashMap<
            reqwest::header::HeaderName,
            reqwest::header::HeaderValue,
        >,
        sse: bool,
        turn_stop: &CancellationToken,
        handler: Arc<input::Handler>,
    ) -> Result<(Self, Service), Fault> {
        use rmcp::transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
        };
        if sse {
            let transport = tokio::select! {
                _ = turn_stop.cancelled() => return Err(unavailable()),
                result = tokio::time::timeout(
                    Duration::from_secs(10),
                    sse::Connection::open(client, &url, headers),
                ) => result.map_err(|_| unavailable())?.map_err(|_| unavailable())?,
            };
            return Self::connect(transport, None, turn_stop, handler).await;
        }
        let mut config = StreamableHttpClientTransportConfig::with_uri(url)
            .custom_headers(headers)
            .reinit_on_expired_session(false);
        config.max_sse_event_size = limits::MAX_MESSAGE;
        Self::connect(
            StreamableHttpClientTransport::with_client(client, config),
            None,
            turn_stop,
            handler,
        )
        .await
    }

    async fn connect<T: Transport<RoleClient> + 'static>(
        transport: T,
        child: Option<Child>,
        turn_stop: &CancellationToken,
        handler: Arc<input::Handler>,
    ) -> Result<(Self, Service), Fault> {
        let (closed, completion) = oneshot::channel();
        let transport = Tracked {
            inner: Some(transport),
            closed: Some(closed),
        };
        // Keep the transport available for task cancellation after a turn stops.
        // Node closes it after the ADK task cleanup has finished.
        let stop = CancellationToken::new();
        let mut owner = Self {
            stop: stop.clone(),
            closed: Some(completion),
            child,
        };
        let result = tokio::select! {
            biased;
            _ = turn_stop.cancelled() => None,
            result = tokio::time::timeout(Duration::from_secs(10), rmcp::service::serve_client_with_ct(handler, transport, stop)) => Some(result),
        };
        match result {
            Some(Ok(Ok(service))) => Ok((owner, service)),
            _ => {
                owner.close().await?;
                Err(unavailable())
            }
        }
    }

    pub(super) async fn close(&mut self) -> Result<(), Fault> {
        self.stop.cancel();
        let transport = match self.closed.take() {
            Some(closed) => matches!(
                // Leave room for the SDK's five-second HTTP session cleanup bound.
                tokio::time::timeout(Duration::from_secs(7), closed).await,
                Ok(Ok(Ok(())))
            ),
            None => true,
        };
        let child = if let Some(child) = &mut self.child {
            tokio::time::timeout(Duration::from_secs(2), child.close()).await
        } else {
            Ok(Ok(()))
        };
        self.child.take();
        match child {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    format!("MCP process cleanup failed: {error}"),
                ));
            }
            Err(_) => {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "MCP process cleanup timed out",
                ));
            }
        }
        if !transport {
            return Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "MCP transport cleanup could not be confirmed",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
