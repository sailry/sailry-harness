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
mod cooldown;
mod exa;
mod network;
mod searxng;
mod tavily;
#[cfg(test)]
mod tests;

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
}

struct State {
    settings: search::Settings,
    cooldowns: BTreeMap<Driver, SystemTime>,
}

pub(crate) struct Searcher {
    state: Mutex<State>,
    active: Mutex<BTreeMap<(NodeId, RequestId), CancellationToken>>,
    #[cfg(any(test, feature = "test-support"))]
    fixtures: Mutex<BTreeMap<Driver, String>>,
}

impl Default for Searcher {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                settings: Default::default(),
                cooldowns: Default::default(),
            }),
            active: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            fixtures: Default::default(),
        }
    }
}

impl Searcher {
    pub fn configure(&self, settings: search::Settings) {
        let mut state = self.state.lock().expect("search state lock is healthy");
        if state.settings != settings {
            state.cooldowns.clear();
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

    pub async fn search(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        plan: Plan,
        input: &search::Request,
        stop: &CancellationToken,
    ) -> Result<search::Page, Fault> {
        input.validate()?;
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
        self.execute(plan, input, &request.stop).await
    }

    async fn execute(
        &self,
        plan: Plan,
        input: &search::Request,
        stop: &CancellationToken,
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
            let endpoint = self.endpoint(driver, &plan.settings);
            let mut page_input = input.clone();
            page_input.limit = match driver {
                Driver::Brave | Driver::Tavily => input.limit.min(20),
                Driver::Bocha => input.limit.min(50),
                Driver::Exa => input.limit.min(100),
                Driver::Searxng => input.limit,
            };
            let input = &page_input;
            let attempt = if driver == Driver::Exa {
                exa::search(&endpoint, &auth, input, stop).await
            } else {
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
                        }
                    }) => result.unwrap_or_else(|_| Err(Failure::unavailable("search service deadline exceeded"))),
                }
            };
            match attempt {
                Ok(mut results) => {
                    results.truncate(input.limit as usize);
                    return Ok(search::Page {
                        driver,
                        results,
                        untrusted: true,
                        fetch_hint: format!(
                            "One provider page, up to {} results; no continuation is exposed. Use web_fetch on relevant URLs for detail",
                            input.limit
                        ),
                    });
                }
                Err(error) => {
                    if let Some(deadline) = error.retry_at {
                        self.cool(driver, plan.settings.revision, deadline);
                    }
                    if !error.temporary || !plan.settings.fallback {
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
        Selection::Auto => Vec::new(),
        Selection::Driver(driver) => vec![driver],
    };
    if matches!(selection, Selection::Auto) || settings.fallback {
        for driver in Driver::ALL {
            if !drivers.contains(&driver) {
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
