//! Session windows count navigation roots, never the rows inside a split.
use super::*;
use sailry_protocol::{NodeId, ProjectId};

pub(super) const INITIAL: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Group {
    Project(NodeId, ProjectId),
    PreviewProject(usize, usize),
}

impl State {
    pub(super) fn sessions_expanded(&self, group: Group) -> bool {
        self.expanded_sessions.contains(&group)
    }

    pub(super) fn session_limit(&self, group: Group) -> usize {
        if self.sessions_expanded(group) {
            usize::MAX
        } else {
            INITIAL
        }
    }
}

impl Shell {
    pub(super) fn more_sessions(
        &self,
        group: Group,
        more: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        more.then(|| {
            Button::new(SharedString::from(format!("sidebar-more-{group:?}")))
                .ghost()
                .small()
                .w_full()
                .justify_start()
                .label(tr(if self.sidebar.sessions_expanded(group) {
                    "sidebar_sessions_less"
                } else {
                    "sidebar_sessions_more"
                }))
                .on_click(cx.listener(move |shell, _, _, cx| {
                    if !shell.sidebar.expanded_sessions.remove(&group) {
                        shell.sidebar.expanded_sessions.insert(group);
                    }
                    cx.notify();
                }))
                .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn keeps_project_expansion_independent() {
        let node = NodeId([1; 32]);
        let project = ProjectId::new();
        let selected = Group::Project(node, project);
        let mut state = State::default();
        state.expanded_sessions.insert(selected);
        assert_eq!(state.session_limit(selected), usize::MAX);
        assert_eq!(
            state.session_limit(Group::Project(node, ProjectId::new())),
            INITIAL
        );
        assert_eq!(
            state.session_limit(Group::Project(NodeId([2; 32]), project)),
            INITIAL
        );
        assert!(state.closed_groups.is_empty());
    }
}
