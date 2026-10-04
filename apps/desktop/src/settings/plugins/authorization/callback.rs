//! Controller-side loopback redirect; OAuth validation and token exchange stay on the Node.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::{io, net::TcpListener as Listener, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

pub(super) struct Callback {
    listener: Listener,
    pub redirect: String,
}

impl Callback {
    pub fn bind() -> Result<Self, Fault> {
        let listener = Listener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(failed)?;
        listener.set_nonblocking(true).map_err(failed)?;
        let redirect = format!("http://{}/callback", listener.local_addr().map_err(failed)?);
        Ok(Self { listener, redirect })
    }

    pub async fn receive(
        self,
        authorization: String,
        text: String,
        stop: CancellationToken,
    ) -> Result<Secret, Fault> {
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
                _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "MCP callback listener closed")),
                result = listener.accept() => result.map_err(failed)?,
            };
            let result = tokio::select! {
                _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "MCP callback listener closed")),
                result = tokio::time::timeout(Duration::from_secs(3), request(stream, &self.redirect, state, &text)) => result,
            };
            if let Ok(Ok(Some(callback))) = result {
                return Ok(callback);
            }
        }
    }
}

async fn request(
    mut stream: TcpStream,
    redirect: &str,
    state: &str,
    text: &str,
) -> io::Result<Option<Secret>> {
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
        && target.starts_with("/callback?")
        && callback.origin() == expected.origin()
        && callback.path() == expected.path()
        && callback.fragment().is_none()
        && states == [state];
    let status = if valid { "200 OK" } else { "400 Bad Request" };
    let body = if valid { text } else { "" };
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
    Ok(valid.then(|| Secret::new(callback.to_string())))
}

fn failed(_: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "MCP callback listener is unavailable",
    )
}
