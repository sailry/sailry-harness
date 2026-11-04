//! Initialization state is independent of the selected package identity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::plugins) enum Initialization {
    #[default]
    Unloaded,
    Loading,
    Published,
    Failed,
    Invalidated,
}

impl Initialization {
    pub(in crate::plugins) fn failed(self) -> bool {
        self == Self::Failed
    }

    pub(in crate::plugins) fn loading(self) -> bool {
        self == Self::Loading
    }

    pub(super) fn invalidate(&mut self) {
        if *self != Self::Unloaded {
            *self = Self::Invalidated;
        }
    }
}

/// One report per unresolved failure; retries keep their original failure cycle.
#[derive(Default)]
pub(super) struct Failures {
    last: Option<(
        sailry_protocol::plugin::Reference,
        sailry_protocol::ErrorCode,
        &'static str,
        bool,
    )>,
}

impl Failures {
    pub(super) fn record(
        &mut self,
        package: &sailry_protocol::plugin::Reference,
        code: sailry_protocol::ErrorCode,
        stage: &'static str,
        system_diagnostic: bool,
    ) -> bool {
        let key = (package.clone(), code, stage, system_diagnostic);
        if self.last.as_ref() == Some(&key) {
            return false;
        }
        self.last = Some(key);
        true
    }

    pub(super) fn clear(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{Failures, Initialization};
    use sailry_protocol::{ErrorCode, plugin::Reference};

    #[test]
    fn failures_and_recovery() {
        for (scenario, check) in [
            (
                "reports_each_unresolved_failure_once",
                (|_scenario: &str| {
                    let package = Reference {
                        name: "example".into(),
                        digest: "a".repeat(64),
                        settings_revision: 1,
                    };
                    let mut failures = Failures::default();
                    assert!(
                        failures.record(&package, ErrorCode::Busy, "view_read", false),
                        "reports each unresolved failure once"
                    );
                    assert!(
                        !failures.record(&package, ErrorCode::Busy, "view_read", false),
                        "reports each unresolved failure once"
                    );
                    assert!(
                        failures.record(&package, ErrorCode::Internal, "view_initialize", false),
                        "reports each unresolved failure once"
                    );
                    assert!(
                        !failures.record(&package, ErrorCode::Internal, "view_initialize", false),
                        "reports each unresolved failure once"
                    );
                    assert!(
                        failures.record(&package, ErrorCode::InvalidRequest, "view_read", false),
                        "reports each unresolved failure once"
                    );
                    assert!(
                        failures.record(&package, ErrorCode::InvalidRequest, "view_read", true),
                        "reports each unresolved failure once"
                    );
                    assert!(
                        !failures.record(&package, ErrorCode::InvalidRequest, "view_read", true),
                        "reports each unresolved failure once"
                    );
                    failures.clear();
                    assert!(
                        failures.record(&package, ErrorCode::Internal, "view_initialize", false),
                        "reports each unresolved failure once"
                    );
                    let updated = Reference {
                        settings_revision: 2,
                        ..package
                    };
                    assert!(
                        failures.record(&updated, ErrorCode::Internal, "view_initialize", false),
                        "reports each unresolved failure once"
                    );
                }) as fn(&str),
            ),
            (
                "disconnected_failures_can_restore",
                (|_scenario: &str| {
                    for mut state in [
                        Initialization::Loading,
                        Initialization::Published,
                        Initialization::Failed,
                    ] {
                        state.invalidate();
                        assert_eq!(
                            state,
                            Initialization::Invalidated,
                            "disconnected failures can restore"
                        );
                        assert!(!state.loading(), "disconnected failures can restore");
                        assert!(!state.failed(), "disconnected failures can restore");
                    }
                    let mut state = Initialization::Unloaded;
                    state.invalidate();
                    assert_eq!(
                        state,
                        Initialization::Unloaded,
                        "disconnected failures can restore"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
