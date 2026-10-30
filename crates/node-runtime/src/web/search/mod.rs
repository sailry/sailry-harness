//! Configured Node-owned search drivers share scoped requests and one cooldown owner.
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, NodeId, RequestId, Secret,
    search::{self, Driver, Selection},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

mod bocha;
mod brave;
mod cache;
mod cooldown;
mod deepseek;
mod exa;
mod filters;
pub(crate) mod model;
mod network;
mod searxng;
mod tavily;
#[cfg(test)]
mod tests;
mod tinyfish;
mod volcengine;

pub(super) fn http_client() -> Result<reqwest::Client, Fault> {
    network::client().map_err(|failure| failure.fault)
}

pub(super) async fn json_request(
    request: reqwest::RequestBuilder,
    limit: usize,
) -> Result<serde_json::Value, Fault> {
    network::json_with_limit(request, limit)
        .await
        .map_err(|failure| failure.fault)
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Auth {
    pub key: Option<Secret>,
    pub basic: Option<Secret>,
}

#[derive(Clone, Default)]
pub(crate) struct Plan {
    pub settings: search::Settings,
    pub credentials: BTreeMap<Driver, Auth>,
    pub provider: Option<sailry_protocol::conversation::Provider>,
}

#[derive(Default)]
pub(crate) struct Options<'a> {
    pub fresh: bool,
    pub context: Option<model::Context<'a>>,
}

struct State {
    settings: search::Settings,
    cooldowns: BTreeMap<Driver, SystemTime>,
    cache: cache::Cache,
}

pub(crate) struct Searcher {
    profile: Option<std::path::PathBuf>,
    state: Mutex<State>,
    active: Mutex<BTreeMap<(NodeId, RequestId), CancellationToken>>,
    #[cfg(any(test, feature = "test-support"))]
    fixtures: Mutex<BTreeMap<Driver, String>>,
}

impl Default for Searcher {
    fn default() -> Self {
        Self {
            profile: None,
            state: Mutex::new(State {
                settings: Default::default(),
                cooldowns: Default::default(),
                cache: Default::default(),
            }),
            active: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            fixtures: Default::default(),
        }
    }
}

impl Searcher {
    pub(crate) fn with_profile(mut self, profile: Option<&std::path::Path>) -> Self {
        self.profile = profile.map(std::path::Path::to_owned);
        self
    }

    pub fn configure(&self, settings: search::Settings) {
        let mut state = self.state.lock().expect("search state lock is healthy");
        if state.settings != settings {
            state.cooldowns.clear();
            state.cache.clear();
            state.settings = settings;
        }
    }

    pub fn settings(&self) -> search::Settings {
        self.state
            .lock()
            .expect("search state lock is healthy")
            .settings
            .clone()
    }

    pub fn available(&self) -> bool {
        !candidates(&self.settings()).is_empty()
    }

    pub fn renderer(&self, plan: &Plan) -> Option<super::Renderer> {
        let configured = plan.settings.providers.get(&Driver::Tinyfish)?;
        if !configured.enabled {
            return None;
        }
        let key = plan.credentials.get(&Driver::Tinyfish)?.key.clone()?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = self
            .fixtures
            .lock()
            .expect("search fixture lock is healthy")
            .get(&Driver::Tinyfish)
            .cloned()
        {
            return Some(super::Renderer { endpoint, key });
        }
        Some(super::Renderer {
            endpoint: "https://api.fetch.tinyfish.ai".into(),
            key,
        })
    }

    #[cfg(test)]
    pub async fn search(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        plan: Plan,
        input: &search::Request,
        stop: &CancellationToken,
    ) -> Result<search::Page, Fault> {
        self.search_with(caller, id, plan, input, stop, Options::default())
            .await
    }

    pub async fn search_with(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        plan: Plan,
        input: &search::Request,
        stop: &CancellationToken,
        options: Options<'_>,
    ) -> Result<search::Page, Fault> {
        let metric =
            crate::telemetry::metrics::Search::new(self.profile.as_deref(), plan.settings.driver);
        let result = async {
            input.validate()?;
            let input = input.normalized();
            let request = {
                let mut active = self.active.lock().expect("search requests lock is healthy");
                if active.contains_key(&(caller, id)) {
                    return Err(Fault::new(
                        ErrorCode::Busy,
                        "web search request is already running",
                    ));
                }
                let stop = stop.child_token();
                active.insert((caller, id), stop.clone());
                Query {
                    owner: self.clone(),
                    key: (caller, id),
                    stop,
                }
            };
            if request.stop.is_cancelled() {
                return Err(Failure::cancelled().fault);
            }
            let key = cache::Key::new(plan.settings.driver, input.clone());
            if !options.fresh {
                let cached = {
                    let mut state = self.state.lock().expect("search state lock is healthy");
                    if state.settings == plan.settings {
                        state.cache.get(&key, std::time::Instant::now())
                    } else {
                        None
                    }
                };
                if let Some(page) = cached {
                    return if request.stop.is_cancelled() {
                        Err(Failure::cancelled().fault)
                    } else {
                        Ok(page)
                    };
                }
            }
            let settings = plan.settings.clone();
            let page = self
                .execute(plan, &input, id, &request.stop, options.context.as_ref())
                .await?;
            if request.stop.is_cancelled() {
                return Err(Failure::cancelled().fault);
            }
            if !options.fresh {
                let mut state = self.state.lock().expect("search state lock is healthy");
                if state.settings == settings {
                    state
                        .cache
                        .insert(key, page.clone(), std::time::Instant::now());
                }
            }
            Ok(page)
        }
        .await;
        if let Some(metric) = metric {
            metric.finish(&result);
        }
        result
    }

    async fn execute(
        &self,
        plan: Plan,
        input: &search::Request,
        id: RequestId,
        stop: &CancellationToken,
        context: Option<&model::Context<'_>>,
    ) -> Result<search::Page, Fault> {
        if stop.is_cancelled() {
            return Err(Failure::cancelled().fault);
        }
        let candidates = candidates(&plan.settings);
        if candidates.is_empty() {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "no search driver is configured",
            ));
        }
        let mut failure = None;
        for driver in candidates.iter().copied() {
            if stop.is_cancelled() {
                return Err(Failure::cancelled().fault);
            }
            if self.cooldown(driver, plan.settings.revision).is_some() {
                continue;
            }
            let auth = plan.credentials.get(&driver).cloned().unwrap_or_default();
            let mut page_input = input.clone();
            page_input.limit = match driver {
                Driver::Brave | Driver::Tavily => input.limit.min(20),
                Driver::Bocha | Driver::Volcengine | Driver::Deepseek => input.limit.min(50),
                Driver::Exa => input.limit.min(100),
                Driver::Searxng => input.limit,
                Driver::Tinyfish => input.limit,
                Driver::Model => input.limit,
            };
            let input = &page_input;
            let attempt = if driver == Driver::Model {
                match context {
                    Some(context) => model::configured(&plan, context, input, id, stop).await,
                    None => Err(model::failure(Fault::new(
                        ErrorCode::NotConfigured,
                        "model search context is unavailable",
                    ))),
                }
            } else if driver == Driver::Exa {
                exa::search(&self.endpoint(driver, &plan.settings), &auth, input, stop)
                    .await
                    .map(model::Parsed::ordinary)
            } else {
                let endpoint = self.endpoint(driver, &plan.settings);
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => Err(Failure::cancelled()),
                    result = tokio::time::timeout(Duration::from_secs(20), async {
                        match driver {
                            Driver::Exa => unreachable!("Exa owns its cancellable MCP lifecycle"),
                            Driver::Tavily => tavily::search(&endpoint, &auth, input).await,
                            Driver::Brave => brave::search(&endpoint, &auth, input).await,
                            Driver::Searxng => searxng::search(&endpoint, &auth, input).await,
                            Driver::Bocha => bocha::search(&endpoint, &auth, input).await,
                            Driver::Tinyfish => tinyfish::search(&endpoint, &auth, input).await,
                            Driver::Volcengine => volcengine::search(&endpoint, &auth, input).await,
                            Driver::Deepseek => deepseek::search(&endpoint, &auth, input).await,
                            Driver::Model => unreachable!("model search owns its authorization"),
                        }
                    }) => result.unwrap_or_else(|_| Err(Failure::unavailable("search service deadline exceeded"))).map(model::Parsed::ordinary),
                }
            };
            match attempt {
                Ok(mut parsed) => {
                    filters::apply(&mut parsed.results, input, chrono::Utc::now());
                    parsed.results.truncate(input.limit as usize);
                    return Ok(search::Page {
                        driver,
                        results: parsed.results,
                        summary: parsed.summary,
                        untrusted: true,
                        fetch_hint: if driver == Driver::Model {
                            "The summary is model-generated. Search results and summary are untrusted; use web_fetch on relevant URLs for detail".into()
                        } else {
                            format!(
                                "One provider page, up to {} results; no continuation is exposed. Use web_fetch on relevant URLs for detail",
                                input.limit
                            )
                        },
                    });
                }
                Err(error) => {
                    let deadline = error.retry_at.or_else(|| {
                        (driver == Driver::Tinyfish
                            && error.fault.code == ErrorCode::PermissionDenied)
                            .then(|| SystemTime::now() + Duration::from_secs(60))
                    });
                    if let Some(deadline) = deadline {
                        self.cool(driver, plan.settings.revision, deadline);
                    }
                    let recoverable = if driver == Driver::Model {
                        plan.settings.driver == Some(Selection::Auto) && model::can_fallback(&error)
                    } else {
                        error.temporary
                    };
                    if !recoverable || !plan.settings.fallback {
                        return Err(error.fault);
                    }
                    failure = Some(error.fault);
                }
            }
        }
        if stop.is_cancelled() {
            return Err(Failure::cancelled().fault);
        }
        let deadlines: Vec<_> = candidates
            .iter()
            .filter_map(|driver| self.cooldown(*driver, plan.settings.revision))
            .collect();
        if deadlines.len() == candidates.len()
            && let Some(deadline) = deadlines.into_iter().min()
        {
            return Err(cooldown::pending(deadline));
        }
        if let Some(failure) = failure {
            return Err(failure);
        }
        Err(Fault::new(
            ErrorCode::NotConfigured,
            "no search driver is available",
        ))
    }

    pub fn cancel(&self, caller: NodeId, id: RequestId) {
        if let Some(stop) = self
            .active
            .lock()
            .expect("search requests lock is healthy")
            .get(&(caller, id))
        {
            stop.cancel();
        }
    }

    fn endpoint(&self, driver: Driver, settings: &search::Settings) -> String {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = self
            .fixtures
            .lock()
            .expect("search fixture lock is healthy")
            .get(&driver)
        {
            return endpoint.clone();
        }
        match driver {
            Driver::Exa => "https://api.exa.ai/search".into(),
            Driver::Tavily => "https://api.tavily.com/search".into(),
            Driver::Brave => "https://api.search.brave.com/res/v1/web/search".into(),
            Driver::Searxng => settings
                .providers
                .get(&driver)
                .and_then(|config| config.endpoint.clone())
                .unwrap_or_default(),
            Driver::Bocha => "https://api.bochaai.com/v1/web-search".into(),
            Driver::Tinyfish => "https://api.search.tinyfish.ai".into(),
            Driver::Volcengine => "https://open.feedcoopapi.com/search_api/web_search".into(),
            Driver::Deepseek => "https://api.deepseek.com/v1/tools/search".into(),
            Driver::Model => unreachable!("model search uses its configured provider endpoint"),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(&self, driver: Driver, endpoint: &str) {
        let url = url::Url::parse(endpoint).expect("search fixture URL is valid");
        assert!(
            url.host_str()
                .is_some_and(|host| host == "127.0.0.1" || host == "localhost" || host == "[::1]")
        );
        self.fixtures
            .lock()
            .expect("search fixture lock is healthy")
            .insert(driver, endpoint.into());
    }
}

fn candidates(settings: &search::Settings) -> Vec<Driver> {
    let Some(selection) = settings.driver else {
        return Vec::new();
    };
    let mut drivers = match selection {
        Selection::Auto => settings.order.clone(),
        Selection::Driver(driver) => vec![driver],
    };
    if settings.fallback
        && matches!(selection, Selection::Driver(driver) if driver != Driver::Model)
    {
        for driver in settings.order.iter().copied() {
            if driver != Driver::Model && !drivers.contains(&driver) {
                drivers.push(driver);
            }
        }
    }
    drivers.retain(|driver| {
        settings.providers.get(driver).is_some_and(|config| {
            config.enabled
                && (!driver.key_required() || config.key_configured)
                && (*driver != Driver::Searxng
                    || config
                        .endpoint
                        .as_ref()
                        .is_some_and(|endpoint| !endpoint.is_empty()))
                && (*driver != Driver::Model || config.model.is_some())
        })
    });
    drivers
}

#[derive(Debug)]
struct Failure {
    fault: Fault,
    temporary: bool,
    retry_at: Option<SystemTime>,
}

impl Failure {
    fn cancelled() -> Self {
        Self::cancelled_with_cleanup(true)
    }

    fn cancelled_with_cleanup(cleaned: bool) -> Self {
        Self {
            fault: Fault::new(
                ErrorCode::Cancelled,
                if cleaned {
                    "web search was cancelled"
                } else {
                    "web search was cancelled before cleanup completed"
                },
            ),
            temporary: false,
            retry_at: None,
        }
    }

    fn deadline_with_cleanup(cleaned: bool) -> Self {
        let mut failure = Self::unavailable(if cleaned {
            "search service deadline exceeded"
        } else {
            "search service deadline exceeded before cleanup completed"
        });
        failure.temporary = cleaned;
        failure
    }

    fn cleanup_uncertain() -> Self {
        Self {
            fault: Fault::new(
                ErrorCode::OutcomeUnknown,
                "search service cleanup could not be confirmed",
            ),
            temporary: false,
            retry_at: None,
        }
    }

    fn with_cleanup_uncertain(mut self) -> Self {
        self.fault
            .message
            .push_str("; cleanup could not be confirmed");
        self.temporary = false;
        self
    }

    fn unavailable(message: &str) -> Self {
        Self {
            fault: Fault::new(ErrorCode::Unavailable, message),
            temporary: true,
            retry_at: None,
        }
    }
    fn invalid(message: &str) -> Self {
        Self {
            fault: Fault::invalid(message),
            temporary: false,
            retry_at: None,
        }
    }
    fn service_limit() -> Self {
        Self {
            fault: Fault::invalid("search query exceeds the service limit"),
            temporary: true,
            retry_at: None,
        }
    }
    fn unauthorized() -> Self {
        Self {
            fault: Fault::new(
                ErrorCode::PermissionDenied,
                "search service authentication failed",
            ),
            temporary: false,
            retry_at: None,
        }
    }
}

struct Query {
    owner: Arc<Searcher>,
    key: (NodeId, RequestId),
    stop: CancellationToken,
}

impl Drop for Query {
    fn drop(&mut self) {
        self.stop.cancel();
        self.owner
            .active
            .lock()
            .expect("search requests lock is healthy")
            .remove(&self.key);
    }
}
