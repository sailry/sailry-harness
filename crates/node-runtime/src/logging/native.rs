use super::{Application, format::Sink};

#[cfg(all(test, target_os = "macos"))]
#[path = "native/tests.rs"]
mod acceptance;

pub(super) struct Native {
    #[cfg(target_os = "macos")]
    logs: Vec<(&'static str, oslog::OsLog)>,
}

impl Native {
    pub fn new(application: Application) -> Self {
        #[cfg(not(target_os = "macos"))]
        let _ = application;
        Self {
            #[cfg(target_os = "macos")]
            logs: super::Area::ALL
                .into_iter()
                .map(|area| {
                    let category = area.name();
                    (
                        category,
                        oslog::OsLog::new(application.subsystem(), category),
                    )
                })
                .collect(),
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn category(area: &str) -> &str {
    if super::Area::ALL
        .into_iter()
        .any(|value| value.name() == area)
    {
        area
    } else {
        "logging"
    }
}

#[cfg(target_os = "macos")]
fn severity(level: tracing::Level) -> oslog::Level {
    match level {
        tracing::Level::TRACE | tracing::Level::DEBUG => oslog::Level::Debug,
        // Essential lifecycle diagnostics should survive after the process exits.
        tracing::Level::INFO => oslog::Level::Default,
        // Apple's warning and error APIs both use Error; Fault is for bugs.
        // The original WARN/ERROR level remains visible in the shared message.
        tracing::Level::WARN | tracing::Level::ERROR => oslog::Level::Error,
    }
}

impl Sink for Native {
    fn emit(&self, level: tracing::Level, area: &str, line: &str) {
        #[cfg(target_os = "macos")]
        if let Some((_, log)) = self.logs.iter().find(|(name, _)| *name == category(area)) {
            log.with_level(severity(level), line);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (level, area, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_known_areas() {
        for area in super::super::Area::ALL {
            assert_eq!(category(area.name()), area.name());
        }
        assert_eq!(category("link"), "link");
        assert_eq!(category("unexpected"), "logging");
        assert_eq!(category(""), "logging");
        assert_eq!(Application::Desktop.subsystem(), "ai.sailry.desktop");
        assert_eq!(Application::Host.subsystem(), "ai.sailry.host");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn maps_levels_without_faults() {
        for (level, expected) in [
            (tracing::Level::TRACE, oslog::Level::Debug),
            (tracing::Level::DEBUG, oslog::Level::Debug),
            (tracing::Level::INFO, oslog::Level::Default),
            (tracing::Level::WARN, oslog::Level::Error),
            (tracing::Level::ERROR, oslog::Level::Error),
        ] {
            assert_eq!(severity(level) as u8, expected as u8);
        }
    }
}
