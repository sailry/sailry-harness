//! The existing SDK transport retains framing, retries and SSE bounds.
use super::*;
use rmcp::{
    model::ClientJsonRpcMessage,
    transport::{
        common::client_side_sse::BoxedSseResponse,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) struct Client {
    inner: reqwest::Client,
    authorization: Authorization,
}

impl Client {
    pub(super) fn new(inner: reqwest::Client, authorization: Authorization) -> Self {
        Self {
            inner,
            authorization,
        }
    }

    async fn prepare(
        &self,
        uri: Arc<str>,
        mut headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(Arc<str>, HashMap<HeaderName, HeaderValue>), StreamableHttpError<reqwest::Error>>
    {
        let mut mapped = uri.to_string();
        for target in &self.authorization.targets {
            mapped = self
                .authorization
                .service
                .resource(target, &uri)
                .map_err(failure)?;
            let value = self
                .authorization
                .service
                .access(target)
                .await
                .map_err(failure)?;
            headers.insert(target.header.clone(), value);
        }
        Ok((Arc::from(mapped), headers))
    }
}

fn failure(error: Fault) -> StreamableHttpError<reqwest::Error> {
    StreamableHttpError::UnexpectedServerResponse(error.message.into())
}

impl StreamableHttpClient for Client {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let (uri, headers) = self.prepare(uri, custom_headers).await?;
        self.inner
            .post_message(uri, message, session_id, auth_header, headers)
            .await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let (uri, headers) = self.prepare(uri, custom_headers).await?;
        self.inner
            .post_message_with_max_sse_event_size(
                uri,
                message,
                session_id,
                auth_header,
                headers,
                max_sse_event_size,
            )
            .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let (uri, headers) = self.prepare(uri, custom_headers).await?;
        self.inner
            .delete_session(uri, session_id, auth_header, headers)
            .await
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxedSseResponse, StreamableHttpError<Self::Error>> {
        let (uri, headers) = self.prepare(uri, custom_headers).await?;
        self.inner
            .get_stream(uri, session_id, last_event_id, auth_header, headers)
            .await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<BoxedSseResponse, StreamableHttpError<Self::Error>> {
        let (uri, headers) = self.prepare(uri, custom_headers).await?;
        self.inner
            .get_stream_with_max_sse_event_size(
                uri,
                session_id,
                last_event_id,
                auth_header,
                headers,
                max_sse_event_size,
            )
            .await
    }
}
