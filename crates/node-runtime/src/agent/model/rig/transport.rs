use futures::StreamExt;
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

    fn prepare<T: Into<Bytes>>(&self, request: Request<T>) -> Result<reqwest::Request, Error> {
        let (parts, body) = request.into_parts();
        let mut endpoint = url::Url::parse(&parts.uri.to_string()).map_err(Error::instance)?;
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
        if matches!(self.api, ModelApi::ChatCompletions | ModelApi::DeepSeek)
            || self.deployment.is_some()
        {
            let mut value = serde_json::from_slice(&body).map_err(Error::instance)?;
            if matches!(self.api, ModelApi::ChatCompletions | ModelApi::DeepSeek) {
                super::schema::chat(&mut value);
            }
            if let Some(deployment) = &self.deployment {
                value["model"] = deployment.clone().into();
            }
            body = serde_json::to_vec(&value).map_err(Error::instance)?.into();
        }
        let mut request = self
            .client
            .request(parts.method, endpoint)
            .headers(headers)
            .body(body)
            .build()
            .map_err(Error::instance)?;
        if let Some(agent) = &self.claude {
            crate::providers::claude::decorate(&mut request, agent)
                .map_err(|error| Error::instance(std::io::Error::other(error.message)))?;
        }
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
            let response = client.execute(request?).await.map_err(Error::instance)?;
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
        async {
            Err(Error::unbuilt(std::io::Error::other(
                "model completion does not use multipart requests",
            )))
        }
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
        async move {
            let response = client.execute(request?).await.map_err(Error::instance)?;
            let mut builder = Response::builder().status(response.status());
            *builder.headers_mut().ok_or(Error::NoHeaders)? = response.headers().clone();
            let stream = response
                .bytes_stream()
                .map(|result| result.map_err(Error::instance));
            Ok(builder.body(Box::pin(stream) as rig_core::http_client::BoxedStream)?)
        }
    }
}
