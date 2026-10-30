//! Controller-side loopback redirect; OAuth validation and token exchange stay on the Node.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::{future::Future, io, net::TcpListener as Listener, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

pub struct Callback {
    listener: Listener,
    pub redirect: String,
}

pub enum Reply {
    Text(String),
    Html(String),
}

impl Callback {
    pub fn bind(path: &str) -> Result<Self, Fault> {
        Self::listen(path, None, false)
    }

    pub fn bind_declared(
        callback: &sailry_protocol::plugin::model_auth::Callback,
    ) -> Result<Self, Fault> {
        Self::listen(
            &callback.path,
            callback.port,
            callback.port.is_some() && callback.allow_localhost,
        )
    }

    fn listen(path: &str, port: Option<u16>, localhost: bool) -> Result<Self, Fault> {
        if port == Some(0) || !path.starts_with('/') || path.contains(['?', '#']) {
            return Err(failed("invalid callback path"));
        }
        let listener =
            Listener::bind((std::net::Ipv4Addr::LOCALHOST, port.unwrap_or(0))).map_err(failed)?;
        listener.set_nonblocking(true).map_err(failed)?;
        let address = listener.local_addr().map_err(failed)?;
        let host = if localhost { "localhost" } else { "127.0.0.1" };
        let redirect = format!("http://{host}:{}{path}", address.port());
        Ok(Self { listener, redirect })
    }

    /// Preserve the endpoint while a request with an uncertain outcome is retried.
    pub fn duplicate(&self) -> Result<Self, Fault> {
        Ok(Self {
            listener: self.listener.try_clone().map_err(failed)?,
            redirect: self.redirect.clone(),
        })
    }

    pub async fn receive<R>(
        self,
        authorization: String,
        reply: R,
        stop: CancellationToken,
    ) -> Result<Secret, Fault>
    where
        R: Future<Output = Reply> + Send + 'static,
    {
        let url = Url::parse(&authorization).map_err(failed)?;
        let states: Vec<_> = url
            .query_pairs()
            .filter(|(name, _)| name == "state")
            .map(|(_, value)| value.into_owned())
            .collect();
        let [state] = states.as_slice() else {
            return Err(failed("authorization state is missing"));
        };
        let listener = TcpListener::from_std(self.listener).map_err(failed)?;
        loop {
            let (stream, _) = tokio::select! {
                _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "OAuth callback listener closed")),
                result = listener.accept() => result.map_err(failed)?,
            };
            let result = tokio::select! {
                _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "OAuth callback listener closed")),
                result = tokio::time::timeout(Duration::from_secs(3), request(stream, &self.redirect, state)) => result,
            };
            if let Ok(Ok(Some((callback, mut stream)))) = result {
                // Let the controller submit the callback before preparing its
                // response. Receipt is not the Node's authorization outcome.
                tokio::spawn(async move {
                    let _ = respond(&mut stream, "200 OK", reply.await).await;
                });
                return Ok(callback);
            }
        }
    }
}

async fn request(
    mut stream: TcpStream,
    redirect: &str,
    state: &str,
) -> io::Result<Option<(Secret, TcpStream)>> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 2048];
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await?;
        if count == 0 || bytes.len() + count > 16 * 1024 {
            return Ok(None);
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let head = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    let method = first.next();
    let target = first.next().unwrap_or_default();
    let expected = Url::parse(redirect).map_err(io::Error::other)?;
    let callback = expected.join(target).map_err(io::Error::other)?;
    let states: Vec<_> = callback
        .query_pairs()
        .filter(|(name, _)| name == "state")
        .map(|(_, value)| value.into_owned())
        .collect();
    let valid = method == Some("GET")
        && target.starts_with(&format!("{}?", expected.path()))
        && callback.origin() == expected.origin()
        && callback.path() == expected.path()
        && callback.fragment().is_none()
        && states == [state];
    if valid {
        Ok(Some((Secret::new(callback.to_string()), stream)))
    } else {
        respond(&mut stream, "400 Bad Request", Reply::Text(String::new())).await?;
        Ok(None)
    }
}

async fn respond(stream: &mut TcpStream, status: &str, reply: Reply) -> io::Result<()> {
    let (content_type, body) = match reply {
        Reply::Text(body) => ("text/plain", body),
        Reply::Html(body) => ("text/html", body),
    };
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
}

fn failed(_: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "OAuth callback listener is unavailable",
    )
}

#[cfg(test)]
#[path = "oauth/tests.rs"]
mod tests;
