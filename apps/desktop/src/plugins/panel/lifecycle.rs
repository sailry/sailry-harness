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
}

/// One report per unresolved failure; retries keep their original failure cycle.
#[derive(Default)]
pub(super) struct Failures {
    last: Option<(
        sailry_protocol::plugin::Reference,
        sailry_protocol::ErrorCode,
        &'static str,
    )>,
}

impl Failures {
    pub(super) fn record(
        &mut self,
        package: &sailry_protocol::plugin::Reference,
        code: sailry_protocol::ErrorCode,
        stage: &'static str,
    ) -> bool {
        let key = (package.clone(), code, stage);
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
    use super::Failures;
    use sailry_protocol::{ErrorCode, plugin::Reference};

    #[test]
    fn reports_each_unresolved_failure_once() {
        let package = Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        };
        let mut failures = Failures::default();
        assert!(failures.record(&package, ErrorCode::Busy, "view_read"));
        assert!(!failures.record(&package, ErrorCode::Busy, "view_read"));
        assert!(failures.record(&package, ErrorCode::Internal, "view_initialize"));
        assert!(!failures.record(&package, ErrorCode::Internal, "view_initialize"));
        failures.clear();
        assert!(failures.record(&package, ErrorCode::Internal, "view_initialize"));
        let updated = Reference {
            settings_revision: 2,
            ..package
        };
        assert!(failures.record(&updated, ErrorCode::Internal, "view_initialize"));
    }
}
