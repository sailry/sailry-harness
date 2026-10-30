//! Explicit temporary-profile collection without a reporter, key, or transport.
use super::{Observation, aggregate::Window};
use crate::telemetry::{Configuration, Control};
use chrono::Utc;
use sailry_protocol::{Fault, telemetry::Settings};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

static REGISTRY: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Capture>>>> = OnceLock::new();

/// An in-memory observer registered before starting an isolated test Node.
/// It never constructs a production telemetry service or sends network requests.
pub struct Collector {
    profile: PathBuf,
    capture: Arc<Capture>,
}

struct Capture {
    control: Arc<Control>,
    data: Mutex<Data>,
}

#[derive(Default)]
struct Data {
    configured: bool,
    window: Option<Window>,
}

impl Collector {
    /// The caller retains ownership of this temporary profile until Node shutdown.
    pub fn register(profile: &tempfile::TempDir) -> Result<Self, Fault> {
        let profile = profile
            .path()
            .canonicalize()
            .map_err(|_| Fault::invalid("collector profile is unavailable"))?;
        let temporary = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| Fault::invalid("temporary directory is unavailable"))?;
        if profile == temporary || !profile.starts_with(&temporary) {
            return Err(Fault::invalid("collector requires a temporary profile"));
        }
        if super::super::PROCESS
            .get()
            .is_some_and(|process| process.profile == profile)
        {
            return Err(Fault::invalid(
                "collector cannot replace the process reporter",
            ));
        }
        let mut registry = REGISTRY
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| Fault::invalid("collector registry is unavailable"))?;
        registry.retain(|_, capture| capture.strong_count() > 0);
        if registry.contains_key(&profile) {
            return Err(Fault::invalid("collector profile is already registered"));
        }
        let capture = Arc::new(Capture {
            control: Arc::new(Control::new(
                crate::logging::Application::Host,
                &Configuration {
                    allowed: true,
                    dsn: None,
                    // Only the configured flag is retained by Control. There is
                    // no key, credential storage or reporting transport here.
                    key: Some(String::new()),
                },
            )),
            data: Mutex::new(Data::default()),
        });
        registry.insert(profile.clone(), Arc::downgrade(&capture));
        Ok(Self { profile, capture })
    }

    /// Drain the same table codec used by the production aggregation window.
    pub fn payloads(&self) -> Vec<Value> {
        let mut data = self
            .capture
            .data
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(window) = data.window.take() else {
            return Vec::new();
        };
        if !self.capture.control.metrics() || window.epoch() != self.capture.control.epoch() {
            return Vec::new();
        }
        window
            .finish(Utc::now())
            .into_iter()
            .filter_map(|batch| serde_json::to_value(batch).ok())
            .collect()
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        if let Some(registry) = REGISTRY.get()
            && let Ok(mut registry) = registry.lock()
            && registry
                .get(&self.profile)
                .and_then(Weak::upgrade)
                .is_some_and(|capture| Arc::ptr_eq(&capture, &self.capture))
        {
            registry.remove(&self.profile);
        }
    }
}

fn lookup(profile: Option<&Path>) -> Option<Arc<Capture>> {
    let profile = profile?.canonicalize().ok()?;
    let mut registry = REGISTRY.get()?.lock().ok()?;
    registry.retain(|_, capture| capture.strong_count() > 0);
    registry.get(&profile)?.upgrade()
}

pub(in crate::telemetry) fn enabled(profile: Option<&Path>) -> Option<bool> {
    lookup(profile).map(|capture| capture.control.metrics())
}

pub(in crate::telemetry) fn control(profile: Option<&Path>) -> Option<Arc<Control>> {
    lookup(profile).map(|capture| capture.control.clone())
}

pub(in crate::telemetry) fn configure(profile: &Path, settings: &Settings) {
    let Some(capture) = lookup(Some(profile)) else {
        return;
    };
    let mut data = capture
        .data
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !capture.control.configure(settings) {
        return;
    }
    data.window = None;
    if !std::mem::replace(&mut data.configured, true) && capture.control.metrics() {
        let mut window = Window::new(
            capture.control.epoch(),
            capture.control.metadata(),
            Utc::now(),
        );
        window.add(Observation::Launch);
        data.window = Some(window);
    }
}

pub(in crate::telemetry) fn record(
    profile: Option<&Path>,
    observation: Observation,
) -> Result<(), Observation> {
    let Some(capture) = lookup(profile) else {
        return Err(observation);
    };
    let epoch = capture.control.epoch();
    store(&capture, &capture.control, epoch, observation);
    Ok(())
}

pub(in crate::telemetry) fn record_at(
    profile: Option<&Path>,
    control: &Arc<Control>,
    epoch: u64,
    observation: Observation,
) -> Result<(), Observation> {
    let Some(capture) = lookup(profile) else {
        return Err(observation);
    };
    store(&capture, control, epoch, observation);
    Ok(())
}

fn store(capture: &Capture, control: &Arc<Control>, epoch: u64, observation: Observation) {
    let mut data = capture
        .data
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if Arc::ptr_eq(&capture.control, control)
        && capture.control.metrics()
        && capture.control.epoch() == epoch
    {
        data.window
            .get_or_insert_with(|| {
                Window::new(
                    capture.control.epoch(),
                    capture.control.metadata(),
                    Utc::now(),
                )
            })
            .add(observation);
    }
}
