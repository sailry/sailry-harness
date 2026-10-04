use crate::tr;
use gpui_kit::SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Verifying,
    Staged,
    Applying,
    RestartPending,
    Failed,
    Loading,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "update_idle",
            Self::Checking => "update_checking",
            Self::Current => "update_current",
            Self::Available => "update_available",
            Self::Downloading => "update_downloading",
            Self::Verifying => "update_verifying",
            Self::Staged => "update_staged",
            Self::Applying => "update_applying",
            Self::RestartPending => "update_restart",
            Self::Failed => "update_failed",
            Self::Loading => "update_loading",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Check,
    Download,
    Apply,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub phase: Phase,
    pub current: SharedString,
    pub target: Option<SharedString>,
    pub progress: Option<u8>,
    pub note: Option<SharedString>,
    pub error: Option<SharedString>,
    pub retry: Action,
}

impl State {
    pub fn sample(scene: usize) -> Self {
        let phase = [
            Phase::Idle,
            Phase::Checking,
            Phase::Current,
            Phase::Available,
            Phase::Downloading,
            Phase::Verifying,
            Phase::Staged,
            Phase::Applying,
            Phase::RestartPending,
            Phase::Failed,
            Phase::Loading,
        ]
        .get(scene)
        .copied()
        .unwrap_or(Phase::Idle);
        Self {
            phase,
            current: "0.1.0-preview".into(),
            target: matches!(
                phase,
                Phase::Available
                    | Phase::Downloading
                    | Phase::Verifying
                    | Phase::Staged
                    | Phase::Applying
                    | Phase::RestartPending
            )
            .then(|| "0.2.0-preview".into()),
            progress: matches!(phase, Phase::Downloading | Phase::Verifying).then_some(45),
            note: (scene == 11).then(|| tr("update_unavailable_note")),
            error: (scene == 9 || scene == 12).then(|| tr("update_error_preview")),
            retry: if scene == 12 {
                Action::Status
            } else {
                Action::Check
            },
        }
    }

    pub fn primary(&self) -> Option<(Action, &'static str)> {
        if self.error.is_some() {
            return Some((
                if self.phase == Phase::Failed {
                    Action::Check
                } else {
                    self.retry
                },
                "update_retry",
            ));
        }
        match self.phase {
            Phase::Idle if self.note.is_some() => None,
            Phase::Idle | Phase::Current => Some((Action::Check, "update_check")),
            Phase::Available => Some((Action::Download, "update_download")),
            Phase::Staged => Some((Action::Apply, "update_apply")),
            Phase::Failed => Some((Action::Check, "update_retry")),
            _ => None,
        }
    }

    pub fn advance(&mut self, expected: &Self) {
        if self != expected {
            return;
        }
        let Some((action, _)) = self.primary() else {
            return;
        };
        self.error = None;
        // Preview buttons move between example snapshots; no updater or polling task runs.
        match action {
            Action::Check => *self = Self::sample(3),
            Action::Download => {
                self.phase = Phase::Staged;
                self.progress = Some(100);
            }
            Action::Apply => {
                self.phase = Phase::RestartPending;
                self.progress = None;
            }
            Action::Status => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_actions_and_stale_clicks() {
        for scene in [1, 4, 5, 7, 8, 10, 11] {
            assert!(State::sample(scene).primary().is_none());
        }
        let mut state = State::sample(0);
        let idle = state.clone();
        state.advance(&idle);
        assert_eq!(state.phase, Phase::Available);
        state.advance(&idle);
        assert_eq!(state.phase, Phase::Available);
        state.advance(&state.clone());
        assert_eq!(state.phase, Phase::Staged);
        state.advance(&state.clone());
        assert_eq!(state.phase, Phase::RestartPending);
    }

    #[test]
    fn status_retry() {
        let mut state = State::sample(7);
        state.error = Some(tr("update_error_preview"));
        state.retry = Action::Status;
        state.advance(&state.clone());
        assert_eq!(state.phase, Phase::Applying);
        assert!(state.error.is_none());
        let mut failed = State::sample(9);
        failed.retry = Action::Apply;
        assert_eq!(failed.primary().unwrap().0, Action::Check);
        failed.advance(&failed.clone());
        assert_eq!(failed.phase, Phase::Available);
    }
}
