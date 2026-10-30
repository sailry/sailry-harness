//! Process-local consent and anonymous reporting identity.
//! A controller connection never changes another process's reporting choices.
use sailry_protocol::{ErrorCode, Fault, telemetry::Settings};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};
use tokio::sync::Notify;

pub(crate) mod metrics;
#[cfg(any(test, feature = "test-support"))]
pub use metrics::fixture::Collector;

static PROCESS: OnceLock<Process> = OnceLock::new();
static OVERRIDE: AtomicBool = AtomicBool::new(true);
const READY: u8 = 1;
const ERRORS: u8 = 2;
const METRICS: u8 = 4;

struct Process {
    profile: PathBuf,
    control: Arc<Control>,
    metrics: metrics::Metrics,
    report: OnceLock<Arc<crate::logging::report::Report>>,
    configured: AtomicBool,
    closing: AtomicBool,
}

#[derive(Clone)]
pub(crate) struct Metadata {
    pub installation_id: String,
    pub release: String,
    pub channel: &'static str,
    pub application: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

pub(crate) struct Control {
    flags: AtomicU8,
    epoch: AtomicU64,
    allowed: AtomicBool,
    errors_configured: bool,
    metrics_configured: bool,
    metadata: Mutex<Metadata>,
    changed: Notify,
}

impl Control {
    fn new(application: crate::logging::Application, config: &Configuration) -> Self {
        Self {
            flags: AtomicU8::new(0),
            epoch: AtomicU64::new(0),
            allowed: AtomicBool::new(config.allowed),
            errors_configured: config.dsn.is_some(),
            metrics_configured: config.key.is_some(),
            metadata: Mutex::new(Metadata {
                installation_id: String::new(),
                release: format!(
                    "sailry@{}+{}",
                    env!("SAILRY_HOST_VERSION"),
                    option_env!("SAILRY_GIT_SHA").unwrap_or("unknown"),
                ),
                channel: if cfg!(debug_assertions) {
                    "dev"
                } else {
                    "beta"
                },
                application: match application {
                    crate::logging::Application::Desktop => "desktop",
                    crate::logging::Application::Host => "host",
                },
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
            }),
            changed: Notify::new(),
        }
    }

    pub(crate) fn permitted(&self) -> bool {
        self.allowed.load(Ordering::Acquire)
            && OVERRIDE.load(Ordering::Acquire)
            && self.flags.load(Ordering::Acquire) & READY != 0
    }

    pub(crate) fn errors(&self) -> bool {
        self.permitted()
            && self.errors_configured
            && self.flags.load(Ordering::Acquire) & ERRORS != 0
    }

    pub(crate) fn metrics(&self) -> bool {
        self.permitted()
            && self.metrics_configured
            && self.flags.load(Ordering::Acquire) & METRICS != 0
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    pub(crate) fn metadata(&self) -> Metadata {
        self.metadata
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn configure(&self, settings: &Settings) -> bool {
        let flags = READY
            | if settings.errors { ERRORS } else { 0 }
            | if settings.metrics { METRICS } else { 0 };
        let mut metadata = self
            .metadata
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.flags.load(Ordering::Acquire) == flags
            && metadata.installation_id == settings.installation_id.to_string()
        {
            return false;
        }
        self.flags.store(0, Ordering::Release);
        metadata.installation_id = settings.installation_id.to_string();
        self.advance();
        self.flags.store(flags, Ordering::Release);
        true
    }

    fn advance(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
    }

    pub(crate) async fn changed(&self, epoch: u64) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.epoch() != epoch {
                return;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture(errors: bool, permitted: bool) -> Arc<Self> {
        let control = Arc::new(Self::new(
            crate::logging::Application::Host,
            &Configuration {
                allowed: permitted,
                dsn: Some("synthetic".into()),
                key: Some("synthetic".into()),
            },
        ));
        control.configure(&Settings {
            errors,
            metrics: true,
            installation_id: uuid::Uuid::from_u128(1),
            ..Settings::default()
        });
        control
    }

    #[cfg(test)]
    pub(crate) fn fixture_set(&self, errors: bool, permitted: bool) {
        self.allowed.store(permitted, Ordering::Release);
        self.advance();
        self.configure(&Settings {
            errors,
            metrics: permitted,
            installation_id: uuid::Uuid::from_u128(1),
            ..Settings::default()
        });
    }

    #[cfg(test)]
    pub(crate) fn fixture_metadata(&self, metadata: Metadata) {
        *self
            .metadata
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = metadata;
        self.advance();
    }
}

struct Configuration {
    allowed: bool,
    dsn: Option<String>,
    key: Option<String>,
}

impl Configuration {
    fn read() -> Self {
        Self::resolve(
            |key| std::env::var(key).ok(),
            option_env!("SAILRY_SENTRY_DSN"),
            option_env!("SAILRY_TELEMETRY_KEY"),
        )
    }

    fn resolve(
        lookup: impl Fn(&str) -> Option<String>,
        compiled_dsn: Option<&str>,
        compiled_key: Option<&str>,
    ) -> Self {
        fn value(runtime: Option<String>, compiled: Option<&str>) -> Option<String> {
            runtime
                .or_else(|| compiled.map(str::to_owned))
                .filter(|value| !value.trim().is_empty())
        }
        Self {
            allowed: lookup("SAILRY_TELEMETRY").as_deref() != Some("off")
                && lookup("DO_NOT_TRACK").as_deref() != Some("1"),
            dsn: value(lookup("SAILRY_SENTRY_DSN"), compiled_dsn),
            key: value(lookup("SAILRY_TELEMETRY_KEY"), compiled_key),
        }
    }
}

/// Overrides process participation without changing saved profile preferences.
/// Environment opt-outs and disabled saved preferences always retain precedence.
pub fn override_enabled(enabled: bool) {
    OVERRIDE.store(enabled, Ordering::Release);
    if let Some(process) = PROCESS.get() {
        process.control.advance();
    }
}

pub(crate) fn start(profile: &Path, application: crate::logging::Application) -> Arc<Control> {
    let process = PROCESS.get_or_init(|| {
        let configuration = Configuration::read();
        let control = Arc::new(Control::new(application, &configuration));
        let key = configuration.allowed.then_some(configuration.key).flatten();
        Process {
            profile: profile
                .canonicalize()
                .unwrap_or_else(|_| profile.to_owned()),
            metrics: metrics::Metrics::new(control.clone(), key),
            control,
            report: OnceLock::new(),
            configured: AtomicBool::new(false),
            closing: AtomicBool::new(false),
        }
    });
    process.control.clone()
}

pub(crate) fn dsn() -> Option<String> {
    let config = Configuration::read();
    config.allowed.then_some(config.dsn).flatten()
}

pub(crate) fn configure(profile: &Path, settings: &Settings) {
    #[cfg(any(test, feature = "test-support"))]
    metrics::fixture::configure(profile, settings);
    let Some(process) = PROCESS.get() else { return };
    if profile.canonicalize().ok().as_deref() == Some(process.profile.as_path()) {
        if process.control.configure(settings) {
            process.metrics.clear();
        }
        if !process.configured.swap(true, Ordering::AcqRel) {
            process.metrics.record(metrics::Observation::Launch);
        }
    }
}

pub(crate) fn attach_report(report: Arc<crate::logging::report::Report>) {
    if let Some(process) = PROCESS.get() {
        let _ = process.report.set(report);
    }
}

pub(crate) fn metrics_enabled(profile: Option<&Path>) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(enabled) = metrics::fixture::enabled(profile) {
        return enabled;
    }
    PROCESS.get().is_some_and(|process| {
        !process.closing.load(Ordering::Acquire)
            && process.control.metrics()
            && profile == Some(process.profile.as_path())
    })
}

/// Consent and identity are captured before an operation starts.
pub(crate) struct Capture {
    profile: PathBuf,
    control: Arc<Control>,
    epoch: u64,
}

impl Capture {
    fn new(profile: &Path, control: Arc<Control>) -> Option<Self> {
        let epoch = control.epoch();
        control.metrics().then(|| Self {
            profile: profile.to_owned(),
            control,
            epoch,
        })
    }

    pub(crate) fn record(&self, observation: metrics::Observation) {
        #[cfg(any(test, feature = "test-support"))]
        let observation = match metrics::fixture::record_at(
            Some(&self.profile),
            &self.control,
            self.epoch,
            observation,
        ) {
            Ok(()) => return,
            Err(observation) => observation,
        };
        if let Some(process) = PROCESS.get()
            && self.profile == process.profile
            && Arc::ptr_eq(&self.control, &process.control)
            && !process.closing.load(Ordering::Acquire)
        {
            process.metrics.record_at(self.epoch, observation);
        }
    }
}

pub(crate) fn capture(profile: Option<&Path>) -> Option<Capture> {
    let profile = profile?;
    #[cfg(any(test, feature = "test-support"))]
    if let Some(control) = metrics::fixture::control(Some(profile)) {
        return Capture::new(profile, control);
    }
    let process = PROCESS.get()?;
    (profile == process.profile.as_path() && !process.closing.load(Ordering::Acquire))
        .then(|| Capture::new(profile, process.control.clone()))
        .flatten()
}

pub(crate) fn record(profile: Option<&Path>, observation: metrics::Observation) {
    #[cfg(any(test, feature = "test-support"))]
    let observation = match metrics::fixture::record(profile, observation) {
        Ok(()) => return,
        Err(observation) => observation,
    };
    if metrics_enabled(profile)
        && let Some(process) = PROCESS.get()
    {
        process.metrics.record(observation);
    }
}

pub(crate) async fn diagnostics() -> Result<(), Fault> {
    let report = PROCESS
        .get()
        .filter(|process| !process.closing.load(Ordering::Acquire))
        .and_then(|process| process.report.get())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::Unavailable,
                "diagnostic reporting is unavailable",
            )
        })?;
    report.diagnostics().await
}

pub(crate) fn shutdown(timeout: std::time::Duration) {
    let Some(process) = PROCESS.get() else { return };
    if process.closing.swap(true, Ordering::AcqRel) {
        return;
    }
    let deadline = std::time::Instant::now() + timeout;
    let (finished, completion) = std::sync::mpsc::sync_channel(1);
    let _ = std::thread::Builder::new()
        .name("sailry-report-shutdown".into())
        .spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(
                    process
                        .metrics
                        .shutdown(tokio::time::Instant::from_std(deadline)),
                );
            }
            if let Some(report) = process.report.get() {
                let _ = report.close(deadline.saturating_duration_since(std::time::Instant::now()));
            }
            let _ = finished.try_send(());
        });
    let _ = completion.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()));
    process.control.flags.store(0, Ordering::Release);
    process.control.advance();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_configuration_disables_both_sinks() {
        let config = Configuration::resolve(|_| None, None, None);
        let control = Control::new(crate::logging::Application::Desktop, &config);
        control.configure(&Settings::default());
        assert!(control.permitted());
        assert!(!control.errors());
        assert!(!control.metrics());
    }

    #[test]
    fn replay_preserves_the_current_window() {
        let control = Control::fixture(true, true);
        let settings = Settings {
            installation_id: uuid::Uuid::from_u128(1),
            ..Settings::default()
        };
        let epoch = control.epoch();
        assert!(!control.configure(&settings));
        assert_eq!(control.epoch(), epoch);
    }

    #[test]
    fn resolves_runtime_overrides_and_opt_outs() {
        for opt_out in ["SAILRY_TELEMETRY", "DO_NOT_TRACK"] {
            let config = Configuration::resolve(
                |key| match key {
                    "SAILRY_SENTRY_DSN" => Some("runtime-dsn".into()),
                    "SAILRY_TELEMETRY_KEY" => Some("runtime-key".into()),
                    key if key == opt_out => {
                        Some(if key == "DO_NOT_TRACK" { "1" } else { "off" }.into())
                    }
                    _ => None,
                },
                Some("compiled-dsn"),
                Some("compiled-key"),
            );
            assert!(!config.allowed);
            assert_eq!(config.dsn.as_deref(), Some("runtime-dsn"));
            assert_eq!(config.key.as_deref(), Some("runtime-key"));
        }
        let empty =
            Configuration::resolve(|_| Some(String::new()), Some("compiled"), Some("compiled"));
        assert!(empty.dsn.is_none());
        assert!(empty.key.is_none());
    }

    #[tokio::test]
    async fn consent_and_identity_change_the_epoch() {
        let control = Control::fixture(true, true);
        let previous = control.epoch();
        assert!(control.errors());
        control.fixture_set(false, false);
        control.changed(previous).await;
        assert!(!control.permitted());
        assert!(!control.errors());
        assert!(!control.metrics());
        let settings = Settings {
            installation_id: uuid::Uuid::from_u128(2),
            ..Settings::default()
        };
        control.configure(&settings);
        assert_eq!(
            control.metadata().installation_id,
            settings.installation_id.to_string()
        );
    }
}
