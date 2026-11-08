use super::unavailable;
use sailry_protocol::Fault;
use std::time::Duration;

pub(crate) mod aliases;
pub(crate) mod matching;
mod parse;
#[cfg(test)]
mod tests;
pub(crate) use parse::Dataset;

const URL: &str = "https://modeldial.com/data/overview.json";
const BYTE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Scores {
    enabled: bool,
    #[cfg(any(test, feature = "test-support"))]
    fixture: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

pub(crate) enum Download {
    Unchanged,
    Content {
        dataset: Dataset,
        etag: Option<String>,
    },
}

impl Default for Scores {
    fn default() -> Self {
        Self {
            enabled: true,
            #[cfg(any(test, feature = "test-support"))]
            fixture: Default::default(),
        }
    }
}

impl Scores {
    pub(crate) fn available(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if self.fixture.lock().unwrap().is_some() {
            return true;
        }
        self.enabled
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn empty() -> Self {
        Self {
            enabled: false,
            fixture: Default::default(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, endpoint: &str) -> Result<(), Fault> {
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| Fault::invalid("model scores fixture URL is invalid"))?;
        if url.scheme() != "http"
            || !url.host_str().is_some_and(|host| {
                host.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            })
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Fault::invalid(
                "model scores fixture requires loopback HTTP",
            ));
        }
        *self.fixture.lock().unwrap() = Some(url.to_string());
        Ok(())
    }

    pub(crate) async fn download(&self, etag: Option<&str>) -> Result<Download, Fault> {
        #[cfg(any(test, feature = "test-support"))]
        let fixture = self.fixture.lock().unwrap().clone();
        #[cfg(any(test, feature = "test-support"))]
        let url = if let Some(fixture) = fixture.as_deref() {
            fixture
        } else if !self.enabled {
            return Ok(Download::Unchanged);
        } else {
            URL
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let url = {
            let _ = self.enabled;
            URL
        };
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Sailry/",
                env!("CARGO_PKG_VERSION"),
                " model-catalog"
            ))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| unavailable("model scores HTTP client failed"))?;
        let mut request = client.get(url);
        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| unavailable("model scores download failed"))?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            if etag.is_none() {
                return Err(unavailable(
                    "model scores returned an unexpected unchanged response",
                ));
            }
            return Ok(Download::Unchanged);
        }
        if !response.status().is_success() {
            return Err(unavailable("model scores request was rejected"));
        }
        if response
            .content_length()
            .is_some_and(|size| size > BYTE_LIMIT as u64)
        {
            return Err(unavailable("model scores exceed their size limit"));
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("model scores response failed"))?
        {
            if chunk.len() > BYTE_LIMIT.saturating_sub(bytes.len()) {
                return Err(unavailable("model scores exceed their size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        tokio::task::spawn_blocking(move || {
            parse::parse(&bytes).map(|dataset| Download::Content { dataset, etag })
        })
        .await
        .map_err(|_| unavailable("model scores parsing failed"))?
    }
}
