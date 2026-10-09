use super::{Fetcher, unavailable};
use hyper_util::client::proxy::matcher::Matcher;
use reqwest::{Proxy, Url};
use sailry_protocol::Fault;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Default)]
struct Settings {
    http: String,
    https: String,
    bypass: String,
}

impl Settings {
    fn matcher(self, environment: impl Fn(&str) -> Option<String>) -> Matcher {
        let value = |upper, lower| environment(upper).or_else(|| environment(lower));
        let all = value("ALL_PROXY", "all_proxy");
        let http = value("HTTP_PROXY", "http_proxy")
            .or_else(|| all.clone())
            .unwrap_or(self.http);
        let https = value("HTTPS_PROXY", "https_proxy")
            .or(all)
            .unwrap_or(self.https);
        let bypass = value("NO_PROXY", "no_proxy").unwrap_or(self.bypass);
        Matcher::builder()
            .http(http)
            .https(https)
            .no(bypass)
            .build()
    }
}

fn configured() -> Matcher {
    #[cfg(target_os = "macos")]
    let settings = macos::settings();
    #[cfg(windows)]
    let settings = windows::settings();
    #[cfg(not(any(target_os = "macos", windows)))]
    let settings = Settings::default();
    settings.matcher(|name| std::env::var(name).ok())
}

pub(super) async fn select(owner: &Fetcher, url: &Url) -> Result<Option<Proxy>, Fault> {
    #[cfg(any(test, feature = "test-support"))]
    let fixture = owner
        .fixtures
        .lock()
        .expect("web fixture lock is healthy")
        .get(url.host_str().expect("validated URL host"))
        .cloned();
    #[cfg(any(test, feature = "test-support"))]
    let matcher = if let Some(fixture) = fixture {
        fixture
            .proxy
            .unwrap_or_else(|| std::sync::Arc::new(Matcher::builder().build()))
    } else {
        std::sync::Arc::new(
            tokio::task::spawn_blocking(configured)
                .await
                .map_err(|_| unavailable("web proxy settings are unavailable"))?,
        )
    };
    #[cfg(not(any(test, feature = "test-support")))]
    let matcher = {
        let _ = owner;
        tokio::task::spawn_blocking(configured)
            .await
            .map_err(|_| unavailable("web proxy settings are unavailable"))?
    };
    let destination = url
        .as_str()
        .parse()
        .map_err(|_| unavailable("invalid web destination"))?;
    let Some(intercept) = matcher.intercept(&destination) else {
        return Ok(None);
    };
    let mut proxy = Proxy::all(intercept.uri().to_string())
        .map_err(|_| unavailable("unsupported web proxy"))?;
    if let Some(auth) = intercept.basic_auth() {
        proxy = proxy.custom_http_auth(auth.clone());
    }
    if let Some((user, password)) = intercept.raw_auth() {
        proxy = proxy.basic_auth(user, password);
    }
    Ok(Some(proxy))
}
