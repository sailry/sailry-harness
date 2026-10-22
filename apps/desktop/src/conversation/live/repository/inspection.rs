//! Read admission and observations are separate; failed reads never classify a directory.
use sailry_protocol::{NodeId, RepositoryKind, WorktreeId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Kind {
    #[default]
    Unknown,
    Repository,
    PlainDirectory,
}

impl Kind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Repository => "repository",
            Self::PlainDirectory => "plain_directory",
        }
    }
}

#[derive(Default)]
pub(super) struct Inspection {
    scope: Option<(NodeId, WorktreeId)>,
    connected: bool,
    generation: u64,
    pending: Option<u64>,
    observed: Option<Kind>,
    reported: bool,
    attempted: bool,
}

impl Inspection {
    pub(super) fn select(&mut self, scope: Option<(NodeId, WorktreeId)>, connected: bool) -> bool {
        if self.scope == scope && self.connected == connected {
            return false;
        }
        self.scope = scope;
        self.connected = connected;
        self.generation += 1;
        self.pending = None;
        self.observed = None;
        self.reported = false;
        self.attempted = false;
        true
    }

    pub(super) fn begin(&mut self, force: bool) -> Option<u64> {
        if self.scope.is_none()
            || !self.connected
            || self.pending.is_some()
            || !force && self.attempted
        {
            return None;
        }
        self.generation += 1;
        self.attempted = true;
        self.pending = Some(self.generation);
        Some(self.generation)
    }

    pub(super) fn finish(&mut self, generation: u64, kind: Option<RepositoryKind>) -> bool {
        if self.pending != Some(generation) {
            return false;
        }
        self.pending = None;
        self.observed = kind.map(|kind| {
            if kind == RepositoryKind::Directory {
                Kind::PlainDirectory
            } else {
                Kind::Repository
            }
        });
        if self.observed.is_some() {
            self.reported = false;
        }
        true
    }

    pub(super) fn report_failure(&mut self) -> bool {
        !std::mem::replace(&mut self.reported, true)
    }

    pub(super) fn kind(&self) -> Kind {
        self.observed.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
