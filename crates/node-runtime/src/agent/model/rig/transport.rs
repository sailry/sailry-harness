use futures::TryStreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use rig_core::http_client::{
    Bytes, Error, HttpClientExt, LazyBody, MultipartForm, Request, Response, StreamingResponse,
};
use sailry_protocol::{
    Authentication, Fault,
    conversation::{ModelApi, Provider},
};

#[derive(Clone)]
pub(super) struct Transport {
    client: reqwest::Client,
    headers: HeaderMap,
    query: Vec<(String, String)>,
    claude: Option<String>,
    api: ModelApi,
    deployment: Option<String>,
}

impl Transport {
    pub(super) fn new(
        client: reqwest::Client,
        provider: &Provider,
        key: &str,
        mut headers: HeaderMap,
        endpoint: &url::Url,
        api: ModelApi,
        deployment: Option<String>,
    ) -> Result<Self, Fault> {
        let credentials = crate::providers::request::Credentials::ApiKey(
            (!key.is_empty()).then(|| sailry_protocol::Secret::new(key.into())),
        );
        for (name, value) in crate::providers::request::headers(provider, &credentials)? {
            if let Some(name) = name {
                headers.entry(name).or_insert(value);
            }
        }
        headers
            .entry(reqwest::header::USER_AGENT)
            .or_insert(HeaderValue::from_static(concat!(
                "Sailry/",
                env!("CARGO_PKG_VERSION")
            )));
        Ok(Self {
            client,
            headers,
            query: endpoint
                .query_pairs()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect(),
            claude: (provider.authentication == Authentication::ClaudeOAuth).then(|| {
                provider
                    .oauth
                    .as_ref()
                    .map_or("", |options| options.user_agent())
                    .to_owned()
            }),
            api,
            deployment,
        })
    }

    fn prepare<T: Into<Bytes>>(
        &self,
        request: Request<T>,
    ) -> Result<reqwest::Request, Box<dyn std::error::Error + Send + Sync>> {
        let (parts, body) = request.into_parts();
        let mut endpoint = url::Url::parse(&parts.uri.to_string())?;
        let mut query = endpoint
            .query_pairs()
            .filter(|(name, _)| name != "key")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        for (key, value) in &self.query {
            query.retain(|(name, _)| name != key);
            query.push((key.clone(), value.clone()));
        }
        endpoint.set_query(None);
        if !query.is_empty() {
            endpoint.query_pairs_mut().extend_pairs(query);
        }
        let mut headers = parts.headers;
        for name in ["authorization", "x-api-key", "x-goog-api-key", "api-key"] {
            headers.remove(name);
        }
        headers.extend(self.headers.clone());
        let mut body = body.into();
        if matches!(
            self.api,
            ModelApi::ChatCompletions
                | ModelApi::DeepSeek
                | ModelApi::Gemini
                | ModelApi::Anthropic
                | ModelApi::Responses
        ) || self.deployment.is_some()
        {
            let mut value = serde_json::from_slice(&body)?;
            super::media::restore(&mut value, self.api)?;
            if matches!(self.api, ModelApi::ChatCompletions | ModelApi::DeepSeek) {
                super::schema::chat(&mut value);
            }
            if self.api == ModelApi::Gemini {
                if let Some(tools) = value.get_mut("tools")
                    && let Some(first) = tools
                        .as_array()
                        .and_then(|tools| (tools.len() == 1).then(|| tools[0].clone()))
                {
                    *tools = first;
                }
                if let Some(system) = value
                    .get_mut("systemInstruction")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    system.remove("role");
                }
            }
            if let Some(deployment) = &self.deployment {
                value["model"] = deployment.clone().into();
            }
            super::wire::prepare(&mut value, self.api);
            body = serde_json::to_vec(&value)?.into();
        }
        let mut request = self
            .client
            .request(parts.method, endpoint)
            .headers(headers)
            .body(body)
            .build()?;
        if let Some(agent) = &self.claude {
            crate::providers::claude::decorate(&mut request, agent)
                .map_err(|error| std::io::Error::other(error.message))?;
        }
        super::super::observation::prepared(
            request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .and_then(|bytes| std::str::from_utf8(bytes).ok()),
        );
        Ok(request)
    }
}

impl HttpClientExt for Transport {
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = Result<Response<LazyBody<U>>, Error>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        let request = self.prepare(request);
        let client = self.client.clone();
        async move {
            let response = client
                .execute(request.map_err(Error::Instance)?)
                .await
                .map_err(Error::instance)?;
            super::super::observation::headers(response.status().as_u16());
            let mut builder = Response::builder().status(response.status());
            *builder.headers_mut().ok_or(Error::NoHeaders)? = response.headers().clone();
            let body: LazyBody<U> =
                Box::pin(
                    async move { response.bytes().await.map(U::from).map_err(Error::instance) },
                );
            Ok(builder.body(body)?)
        }
    }

    fn send_multipart<U>(
        &self,
        _: Request<MultipartForm>,
    ) -> impl Future<Output = Result<Response<LazyBody<U>>, Error>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(Error::unbuilt(std::io::Error::other(
            "model completion does not use multipart requests",
        ))))
    }

    fn send_streaming<T>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = Result<StreamingResponse, Error>> + Send
    where
        T: Into<Bytes> + Send,
    {
        let request = self.prepare(request);
        let client = self.client.clone();
        let api = self.api;
        async move {
            let request = request.map_err(Error::Instance)?;
            let replay = (api == ModelApi::Responses)
                .then(|| request.try_clone())
                .flatten();
            let mut response = client.execute(request).await.map_err(Error::instance)?;
            if response.status() == reqwest::StatusCode::BAD_REQUEST
                && let Some(mut request) = replay
            {
                let status = response.status();
                let headers = response.headers().clone();
                let bytes = response.bytes().await.map_err(Error::instance)?;
                let error = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
                let rejected =
                    error.contains("encrypted content") && error.contains("could not be verified");
                if rejected {
                    let mut body: serde_json::Value = serde_json::from_slice(
                        request
                            .body()
                            .and_then(reqwest::Body::as_bytes)
                            .unwrap_or_default(),
                    )
                    .map_err(Error::instance)?;
                    if let Some(input) = body["input"].as_array_mut() {
                        input.retain(|item| item["type"] != "reasoning");
                    }
                    let bytes = serde_json::to_vec(&body).map_err(Error::instance)?;
                    super::super::observation::prepared(std::str::from_utf8(&bytes).ok());
                    request
                        .headers_mut()
                        .remove(reqwest::header::CONTENT_LENGTH);
                    *request.body_mut() = Some(bytes.into());
                    response = client.execute(request).await.map_err(Error::instance)?;
                } else {
                    super::super::observation::headers(status.as_u16());
                    let mut builder = Response::builder().status(status);
                    *builder.headers_mut().ok_or(Error::NoHeaders)? = headers;
                    let stream = futures::stream::once(async { Ok(bytes) });
                    return Ok(
                        builder.body(Box::pin(stream) as rig_core::http_client::BoxedStream)?
                    );
                }
            }
            super::super::observation::headers(response.status().as_u16());
            let mut builder = Response::builder().status(response.status());
            *builder.headers_mut().ok_or(Error::NoHeaders)? = response.headers().clone();
            let responses =
                api == ModelApi::Responses && response.status() == reqwest::StatusCode::OK;
            if responses {
                builder.headers_mut().ok_or(Error::NoHeaders)?.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
            }
            let stream = response.bytes_stream().map_err(Error::instance);
            let stream = Box::pin(stream) as rig_core::http_client::BoxedStream;
            let stream = if responses {
                super::responses::framing::stream(stream)
            } else {
                stream
            };
            Ok(builder.body(stream)?)
        }
    }
}
