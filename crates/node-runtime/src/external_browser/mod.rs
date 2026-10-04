//! Execution-Node ownership of ADK browser sessions and their isolated profiles.
mod bundle;
mod driver;
mod execute;
mod files;
mod operations;
#[cfg(test)]
mod tests;
pub(crate) use execute::{Call, validate};

use adk_browser::{BrowserConfig, BrowserSession};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, SessionId};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedMutexGuard};

pub(crate) struct Browsers {
    profile: Option<PathBuf>,
    sessions: Mutex<HashMap<SessionId, Arc<Mutex<Option<Session>>>>>,
    installation: Mutex<()>,
    pub stop: CancellationToken,
}

pub(crate) struct Session {
    pub browser: Arc<BrowserSession>,
    pub directory: PathBuf,
    driver: driver::Driver,
}

impl Browsers {
    pub fn new(profile: Option<PathBuf>) -> Self {
        Self {
            profile,
            sessions: Mutex::new(HashMap::new()),
            installation: Mutex::new(()),
            stop: CancellationToken::new(),
        }
    }

    pub fn available(&self) -> bool {
        self.profile.is_some() && bundle::platform().is_some() && !self.stop.is_cancelled()
    }

    pub async fn acquire(
        &self,
        id: SessionId,
        stop: &CancellationToken,
    ) -> Result<OwnedMutexGuard<Option<Session>>, Fault> {
        if !self.available() {
            return Err(unavailable(
                "external browser is unavailable on the execution Node",
            ));
        }
        let slot = {
            let mut sessions = self.sessions.lock().await;
            sessions.retain(|_, slot| {
                Arc::strong_count(slot) > 1
                    || slot.try_lock().map_or(true, |session| session.is_some())
            });
            if !sessions.contains_key(&id) && sessions.len() >= 4 {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "close an external browser session before starting another",
                ));
            }
            sessions.entry(id).or_default().clone()
        };
        let operation = async {
            let mut slot = slot.lock_owned().await;
            if slot.is_none() {
                let profile = self.profile.as_ref().expect("browser storage is available");
                let bundle = {
                    let _installation = self.installation.lock().await;
                    bundle::install(profile, stop).await?
                };
                let directory = profile.join("browser/sessions").join(id.to_string());
                tokio::fs::create_dir_all(directory.join("downloads"))
                    .await
                    .map_err(io_error)?;
                tokio::fs::create_dir_all(directory.join("profile"))
                    .await
                    .map_err(io_error)?;
                let (driver, url) = driver::Driver::start(&bundle, stop).await?;
                let mut config = BrowserConfig::default()
                    .webdriver_url(url)
                    .viewport(1280, 900)
                    .page_load_timeout(25)
                    .add_arg(format!(
                        "--user-data-dir={}",
                        path_text(&directory.join("profile"))?
                    ))
                    .add_arg("--no-first-run")
                    .add_arg("--no-default-browser-check");
                config.require_explicit_start = true;
                config.implicit_wait_secs = 0;
                config.script_timeout_secs = 15;
                config
                    .chrome_options
                    .insert("binary".into(), path_text(&bundle.browser)?.into());
                config.chrome_options.insert(
                    "prefs".into(),
                    serde_json::json!({
                        "download.default_directory": path_text(&directory.join("downloads"))?,
                        "download.prompt_for_download": false,
                        "download.directory_upgrade": true,
                    }),
                );
                let browser = Arc::new(BrowserSession::new(config));
                browser.start().await.map_err(|error| {
                    unavailable(&format!("managed browser startup failed: {error}"))
                })?;
                *slot = Some(Session {
                    browser,
                    directory,
                    driver,
                });
            }
            Ok(slot)
        };
        tokio::select! {
            _ = stop.cancelled() => Err(unavailable("external browser startup cancelled")),
            _ = self.stop.cancelled() => Err(unavailable("execution Node is shutting down")),
            result = operation => result,
        }
    }

    pub async fn close(&self, id: SessionId) -> Result<(), Fault> {
        let mut sessions = self.sessions.lock().await;
        let slot = sessions.remove(&id);
        if let Some(slot) = slot
            && let Some(session) = slot.lock().await.take()
        {
            session.close().await?;
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), Fault> {
        self.stop.cancel();
        let slots: Vec<_> = self
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, slot)| slot)
            .collect();
        let mut failure = None;
        for slot in slots {
            if let Some(session) = slot.lock().await.take()
                && let Err(error) = session.close().await
            {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

impl Session {
    pub async fn close(self) -> Result<(), Fault> {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.browser.stop()).await;
        self.driver.close().await
    }
}

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
fn io_error(error: std::io::Error) -> Fault {
    unavailable(&format!("managed browser file operation failed: {error}"))
}

fn path_text(path: &std::path::Path) -> Result<&str, Fault> {
    path.to_str()
        .ok_or_else(|| unavailable("managed browser requires UTF-8 paths"))
}
