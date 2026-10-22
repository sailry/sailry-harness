//! One serialized native idle-sleep request controlled by the Node setting.
use sailry_protocol::power::Settings;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;
#[cfg(target_os = "windows")]
mod windows;

trait Backend: Send {
    fn supported(&self) -> bool {
        true
    }
    fn held(&self) -> bool;
    fn acquire(&mut self) -> Result<(), i64>;
    fn release(&mut self) -> Result<(), i64>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub enabled: bool,
    pub acquired: bool,
    pub supported: bool,
    pub closed: bool,
    pub error: Option<i64>,
}

struct State {
    settings: Settings,
    status: Status,
    backend: Box<dyn Backend>,
    release_failed: bool,
}

pub(crate) struct Service {
    state: Mutex<State>,
    failed: Box<dyn Fn(i64) + Send + Sync>,
}

impl Service {
    pub(crate) fn new(failed: impl Fn(i64) + Send + Sync + 'static) -> Arc<Self> {
        #[cfg(target_os = "macos")]
        let backend = Box::new(macos::Native::default());
        #[cfg(target_os = "windows")]
        let backend = Box::new(windows::Native::default());
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let backend = Box::new(Unsupported);
        Self::with_backend(backend, Box::new(failed))
    }

    fn with_backend(
        backend: Box<dyn Backend>,
        failed: Box<dyn Fn(i64) + Send + Sync>,
    ) -> Arc<Self> {
        let supported = backend.supported();
        Arc::new(Self {
            state: Mutex::new(State {
                settings: Settings::default(),
                status: Status {
                    enabled: true,
                    acquired: false,
                    supported,
                    closed: false,
                    error: None,
                },
                backend,
                release_failed: false,
            }),
            failed,
        })
    }

    pub(crate) fn apply_settings(&self, settings: Settings) {
        self.update(|state| {
            // Replayed durable results and delayed readers cannot restore old policy.
            if settings.revision >= state.settings.revision {
                state.status.enabled = settings.prevent_sleep;
                state.settings = settings;
            }
        });
    }

    pub(crate) fn status(&self) -> Status {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status
            .clone()
    }

    pub(crate) fn close(&self) {
        self.update(|state| state.status.closed = true);
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        let failure = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            change(&mut state);
            let status = &state.status;
            let needed = status.enabled && !status.closed;
            let result = if !needed && status.acquired && !state.release_failed {
                let result = state.backend.release();
                state.release_failed = result.is_err();
                result
            } else if needed && !status.acquired && status.supported && status.error.is_none() {
                state.backend.acquire()
            } else {
                Ok(())
            };
            state.status.acquired = state.backend.held();
            match result {
                Err(code) if state.status.error.is_none() => {
                    state.status.error = Some(code);
                    Some(code)
                }
                _ => None,
            }
        };
        // Never call external observers while holding the service lock.
        if let Some(code) = failure {
            (self.failed)(code);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct Unsupported;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Backend for Unsupported {
    fn supported(&self) -> bool {
        false
    }
    fn held(&self) -> bool {
        false
    }
    fn acquire(&mut self) -> Result<(), i64> {
        unreachable!("unsupported power backend")
    }
    fn release(&mut self) -> Result<(), i64> {
        unreachable!("unsupported power backend")
    }
}
