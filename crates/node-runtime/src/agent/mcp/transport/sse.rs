//! Legacy HTTP+SSE framing over the SDK's bounded HTTP and event-stream helpers.
use super::*;
use futures::StreamExt;
use reqwest::{
    Client, Url,
    header::{HeaderName, HeaderValue},
};
use rmcp::transport::{
    common::client_side_sse::BoxedSseResponse,
    streamable_http_client::{StreamableHttpClient, StreamableHttpPostResponse},
};
use std::collections::HashMap;
#[cfg(test)]
mod tests;

pub(super) struct Connection<C = Client> {
    client: C,
    endpoint: Arc<str>,
    headers: HashMap<HeaderName, HeaderValue>,
    stream: Option<BoxedSseResponse>,
    stop: CancellationToken,
}

impl<C: StreamableHttpClient> Connection<C> {
    pub(super) async fn open(
        client: C,
        url: &str,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> io::Result<Self> {
        let origin = Url::parse(url).map_err(|_| failure())?;
        let mut stream = client
            .get_stream_with_max_sse_event_size(
                url.into(),
                None,
                None,
                None,
                headers.clone(),
                limits::MAX_MESSAGE,
            )
            .await
            .map_err(|_| failure())?;
        let endpoint = loop {
            let event = stream
                .next()
                .await
                .ok_or_else(failure)?
                .map_err(|_| failure())?;
            match event.event.as_deref() {
                Some("endpoint") => {
                    let endpoint = origin
                        .join(event.data.as_deref().ok_or_else(failure)?)
                        .map_err(|_| failure())?;
                    // The server cannot redirect the execution Node's private headers.
                    if endpoint.origin() != origin.origin()
                        || !endpoint.username().is_empty()
                        || endpoint.password().is_some()
                        || endpoint.fragment().is_some()
                    {
                        return Err(failure());
                    }
                    break endpoint.as_str().into();
                }
                Some("message") => return Err(failure()),
                _ => {}
            }
        };
        Ok(Self {
            client,
            endpoint,
            headers,
            stream: Some(stream),
            stop: CancellationToken::new(),
        })
    }
}

impl<C> Drop for Connection<C> {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl<C: StreamableHttpClient + Sync> Transport<RoleClient> for Connection<C> {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let (client, endpoint, headers, stop) = (
            self.client.clone(),
            self.endpoint.clone(),
            self.headers.clone(),
            self.stop.clone(),
        );
        async move {
            let response = tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(failure()),
                result = client.post_message_with_max_sse_event_size(
                    endpoint, item, None, None, headers, limits::MAX_MESSAGE,
                ) => result,
            };
            match response {
                Ok(StreamableHttpPostResponse::Accepted) => Ok(()),
                _ => {
                    stop.cancel();
                    Err(failure())
                }
            }
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        loop {
            let event = tokio::select! {
                biased;
                _ = self.stop.cancelled() => None,
                event = self.stream.as_mut()?.next() => event,
            };
            match event {
                Some(Ok(event)) if event.event.as_deref() == Some("message") => {
                    if let Some(message) =
                        event.data.and_then(|data| serde_json::from_str(&data).ok())
                    {
                        return Some(message);
                    }
                    break;
                }
                // A second endpoint must never replace an initialized connection.
                Some(Ok(event)) if event.event.as_deref() != Some("endpoint") => continue,
                _ => break,
            }
        }
        self.stop.cancel();
        self.stream.take();
        None
    }

    async fn close(&mut self) -> io::Result<()> {
        self.stop.cancel();
        self.stream.take();
        Ok(())
    }
}
