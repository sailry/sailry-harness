//! Desktop navigation is projected from shared resources and local split ancestry.
use super::*;
use crate::panes::Target;
use sailry_protocol::ProjectId;
use std::collections::BTreeMap;

pub(crate) struct Entry {
    pub target: Target,
    pub project: Option<ProjectId>,
    pub group: Option<Target>,
    pub folder: bool,
    pub last: bool,
    pub ancestors: Vec<bool>,
}

impl Entry {
    pub fn indent(&self) -> Pixels {
        px(30. + self.ancestors.len() as f32 * 18.)
    }

    pub fn guides(&self, id: String, cx: &App) -> Div {
        let depth = self.ancestors.len();
        super::branch(id, self.last, px(30.), cx).children(
            self.ancestors
                .iter()
                .enumerate()
                .filter(|&(_level, continues)| *continues)
                .map(|(level, _continues)| {
                    div()
                        .absolute()
                        .left(px((level as f32 - depth as f32) * 18.))
                        .top_0()
                        .bottom_0()
                        .border_l_1()
                        .border_dashed()
                        .border_color(cx.theme().sidebar_foreground.opacity(0.35))
                }),
        )
    }
}

fn project(
    entries: &[(Target, Option<ProjectId>, Option<usize>)],
    parent: impl Fn(Target) -> Option<Target>,
    order: impl Fn(Target) -> Vec<Target>,
) -> Vec<Entry> {
    let visible: BTreeMap<_, _> = entries
        .iter()
        .map(|(target, _, rank)| (*target, *rank))
        .collect();
    let mut children: BTreeMap<Target, Vec<Target>> = BTreeMap::new();
    let mut roots: BTreeMap<Option<ProjectId>, Vec<Target>> = BTreeMap::new();
    for (target, project, _) in entries {
        if let Some(owner) = parent(*target).filter(|owner| visible.contains_key(owner)) {
            children.entry(owner).or_default().push(*target);
        } else {
            roots.entry(*project).or_default().push(*target);
        }
    }
    fn rank(
        target: Target,
        visible: &BTreeMap<Target, Option<usize>>,
        children: &BTreeMap<Target, Vec<Target>>,
        ranks: &mut BTreeMap<Target, Option<usize>>,
    ) -> Option<usize> {
        if let Some(rank) = ranks.get(&target) {
            return *rank;
        }
        let mut newest = visible[&target];
        if let Some(descendants) = children.get(&target) {
            for child in descendants {
                if let Some(child) = rank(*child, visible, children, ranks) {
                    newest = Some(newest.map_or(child, |current| current.min(child)));
                }
            }
        }
        ranks.insert(target, newest);
        newest
    }
    let mut ranks = BTreeMap::new();
    for target in visible.keys() {
        rank(*target, &visible, &children, &mut ranks);
    }
    for siblings in roots.values_mut().chain(children.values_mut()) {
        // Keep terminal-only entries first; conversation groups follow their newest member.
        siblings.sort_by_key(|target| {
            let rank = ranks[target];
            (rank.is_some(), rank)
        });
    }
    fn descendants(
        target: Target,
        children: &BTreeMap<Target, Vec<Target>>,
        out: &mut Vec<Target>,
    ) {
        if let Some(members) = children.get(&target) {
            for member in members {
                out.push(*member);
                descendants(*member, children, out);
            }
        }
    }
    let mut rows = Vec::new();
    for (project, targets) in roots {
        for (index, target) in targets.iter().enumerate() {
            let last = index + 1 == targets.len();
            let mut members = vec![*target];
            descendants(*target, &children, &mut members);
            let ordered = order(*target);
            members.sort_by_key(|member| {
                ordered
                    .iter()
                    .position(|item| item == member)
                    .unwrap_or(usize::MAX)
            });
            let group = (members.len() > 1).then_some(*target);
            rows.push(Entry {
                target: members[0],
                project,
                group,
                folder: group.is_some(),
                last,
                ancestors: Vec::new(),
            });
            if group.is_none() {
                continue;
            }
            // Keep terminal pane slots while conversation rows follow the shared order.
            let mut sessions: Vec<_> = members
                .iter()
                .copied()
                .filter(|member| visible[member].is_some())
                .collect();
            sessions.sort_by_key(|member| visible[member]);
            let mut sessions = sessions.into_iter();
            for member in &mut members {
                if visible[member].is_some() {
                    *member = sessions.next().unwrap();
                }
            }
            for (index, member) in members.iter().enumerate() {
                rows.push(Entry {
                    target: *member,
                    project,
                    group,
                    folder: false,
                    last: index + 1 == members.len(),
                    ancestors: vec![!last],
                });
            }
        }
    }
    rows
}

impl Shell {
    pub(crate) fn live_resource_rows(
        &self,
        project_id: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        let Some(snapshot) = &live.view.snapshot else {
            return div().into_any_element();
        };
        let node = live.selected;
        let terminals: Vec<_> = snapshot
            .terminals
            .iter()
            .filter_map(|info| {
                if info.status == sailry_protocol::terminal::Status::Closed {
                    return None;
                }
                let tree = snapshot
                    .worktrees
                    .iter()
                    .find(|tree| Some(tree.id) == info.worktree)?;
                Some((Target::Terminal(node, tree.id, info.id), tree.project, info))
            })
            .collect();
        let mut sessions: Vec<_> = snapshot
            .sessions
            .iter()
            .filter_map(|session| {
                (session.config.resource.is_none()
                    && (session.project.is_some() || session.config.assistant.is_none())
                    && session.delegation.is_none()
                    && !session.archived)
                    .then_some((Target::Session(node, session.id), session.project, session))
            })
            .collect();
        sessions.sort_by_key(|(_, _, session)| {
            !crate::preferences::sessions::get(node, session.id, cx).pinned
        });
        let entries: Vec<_> = terminals
            .iter()
            .map(|(target, project, _)| (*target, *project, None))
            // The shared Node projection supplies creation order and saved manual overrides.
            .chain(
                sessions
                    .iter()
                    .enumerate()
                    .map(|(rank, (target, project, _))| (*target, *project, Some(rank))),
            )
            .collect();
        let rows = project(
            &entries,
            |target| self.splits.read(cx).parent(target),
            |target| self.splits.read(cx).ordered_members(target, cx),
        );
        v_flex()
            .gap_0p5()
            .children(
                rows.iter()
                    .filter(|row| {
                        row.project == project_id
                            && row
                                .group
                                .is_none_or(|group| row.folder || self.sidebar.group_open(group))
                    })
                    .map(|row| match row.target {
                        Target::Session(..) => {
                            let (_, _, session) = sessions
                                .iter()
                                .find(|(target, _, _)| *target == row.target)
                                .unwrap();
                            if row.folder {
                                self.split_folder(row, crate::activity::title(session), cx)
                            } else {
                                self.live_session_row(
                                    session,
                                    super::sessions::Placement::Project(row),
                                    cx,
                                )
                            }
                        }
                        Target::Terminal(..) => {
                            let (_, owner, info) = terminals
                                .iter()
                                .find(|(target, _, _)| *target == row.target)
                                .unwrap();
                            let index = terminals
                                .iter()
                                .filter(|(_, project, _)| project == owner)
                                .position(|(target, _, _)| *target == row.target)
                                .unwrap();
                            if row.folder {
                                self.split_folder(
                                    row,
                                    self.terminal_title(node, info.worktree.unwrap(), info.id, cx),
                                    cx,
                                )
                            } else {
                                self.live_terminal_row(*owner, index, info, row, cx)
                            }
                        }
                        Target::Draft => unreachable!(),
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::{NodeId, SessionId, TerminalId, WorktreeId};

    #[test]
    fn split_and_detach_order() {
        let node = NodeId([1; 32]);
        let a = Target::Session(node, SessionId::new());
        let b = Target::Session(node, SessionId::new());
        let terminal = Target::Terminal(node, WorktreeId::new(), TerminalId::new());
        let first = Some(ProjectId::new());
        let second = Some(ProjectId::new());
        let entries = [
            (terminal, second, None),
            (a, first, Some(1)),
            (b, second, Some(2)),
        ];
        let parents = BTreeMap::from([(b, a), (terminal, b)]);
        let rows = project(
            &entries,
            |target| parents.get(&target).copied(),
            |_| Vec::new(),
        );
        assert_eq!(
            rows.iter().map(|row| row.target).collect::<Vec<_>>(),
            [a, a, b, terminal]
        );
        assert!(rows.iter().all(|row| row.project == first));
        assert_eq!(
            rows.iter()
                .map(|row| row.ancestors.len())
                .collect::<Vec<_>>(),
            [0, 1, 1, 1]
        );
        assert!(rows[0].folder);
        assert!(
            rows[1..]
                .iter()
                .all(|row| !row.folder && row.group == Some(a))
        );
        let detached = project(&entries, |_| None, |_| Vec::new());
        assert_eq!(
            detached
                .iter()
                .filter(|row| row.project == second)
                .map(|row| row.target)
                .collect::<Vec<_>>(),
            [terminal, b]
        );
        assert!(detached.iter().all(|row| row.ancestors.is_empty()));
        assert!(
            detached
                .iter()
                .all(|row| !row.folder && row.group.is_none())
        );
    }

    #[test]
    fn hidden_parents() {
        let node = NodeId([1; 32]);
        let a = Target::Session(node, SessionId::new());
        let b = Target::Session(node, SessionId::new());
        let rows = project(
            &[(b, Some(ProjectId::new()), Some(1))],
            |_| Some(a),
            |_| Vec::new(),
        );
        assert_eq!(rows[0].target, b);
        assert!(rows[0].ancestors.is_empty());
        assert!(!rows[0].folder);
        assert!(rows[0].group.is_none());
    }

    #[test]
    fn conversation_order_preserves_folder_owner() {
        let node = NodeId([1; 32]);
        let owner = Some(ProjectId::new());
        let [a, b, c] = std::array::from_fn(|_| Target::Session(node, SessionId::new()));
        let entries = [
            (a, owner, Some(0)),
            (b, owner, Some(1)),
            (c, owner, Some(2)),
        ];
        let parents = BTreeMap::from([(b, a), (c, b)]);
        let rows = project(
            &entries,
            |target| parents.get(&target).copied(),
            |_| vec![c, a, b],
        );
        assert_eq!(
            rows.iter().map(|row| row.target).collect::<Vec<_>>(),
            [c, a, b, c]
        );
        assert!(rows[0].folder);
        assert!(rows.iter().all(|row| row.group == Some(a)));
        assert!(
            rows[1..]
                .iter()
                .all(|row| !row.folder && row.ancestors.len() == 1)
        );
    }

    #[test]
    fn stable_session_order() {
        let node = NodeId([1; 32]);
        let owner = Some(ProjectId::new());
        let [a, b, c, d] = std::array::from_fn(|_| Target::Session(node, SessionId::new()));
        let entries = [
            (a, owner, Some(30)),
            (b, owner, Some(10)),
            (c, owner, Some(20)),
            (d, owner, Some(20)),
        ];
        let rows = project(&entries, |_| None, |_| Vec::new());
        assert_eq!(
            rows.iter().map(|row| row.target).collect::<Vec<_>>(),
            [b, c, d, a]
        );
        let reversed: Vec<_> = entries.into_iter().rev().collect();
        assert_eq!(
            project(&reversed, |_| None, |_| Vec::new())
                .iter()
                .map(|row| row.target)
                .collect::<Vec<_>>(),
            [b, d, c, a]
        );
    }

    #[test]
    fn terminal_source_order() {
        let node = NodeId([1; 32]);
        let owner = Some(ProjectId::new());
        let mut terminals: [Target; 2] =
            std::array::from_fn(|_| Target::Terminal(node, WorktreeId::new(), TerminalId::new()));
        terminals.sort_by(|left, right| right.cmp(left));
        let [first, second] = terminals;
        let session = Target::Session(node, SessionId::new());
        let entries = [
            (first, owner, None),
            (session, owner, Some(0)),
            (second, owner, None),
        ];
        let rows = project(&entries, |_| None, |_| Vec::new());
        assert_eq!(
            rows.iter().map(|row| row.target).collect::<Vec<_>>(),
            [first, second, session]
        );
    }

    #[test]
    fn descendant_order() {
        let node = NodeId([1; 32]);
        let owner = Some(ProjectId::new());
        let [a, b, c, d] = std::array::from_fn(|_| Target::Session(node, SessionId::new()));
        let terminal = Target::Terminal(node, WorktreeId::new(), TerminalId::new());
        let nested = Target::Terminal(node, WorktreeId::new(), TerminalId::new());
        let mut entries = [
            (a, owner, Some(90)),
            (b, owner, Some(40)),
            (c, owner, Some(20)),
            (d, owner, Some(70)),
            (terminal, owner, None),
            (nested, owner, None),
        ];
        let parents = BTreeMap::from([(c, a), (d, a), (nested, c)]);
        let rows = project(
            &entries,
            |target| parents.get(&target).copied(),
            |_| Vec::new(),
        );
        assert_eq!(
            rows.iter().map(|row| row.target).collect::<Vec<_>>(),
            [terminal, a, c, d, nested, a, b]
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.ancestors.len())
                .collect::<Vec<_>>(),
            [0, 0, 1, 1, 1, 1, 0]
        );
        assert!(!rows[2].last);
        assert!(rows[5].last);
        entries[1].2 = Some(10);
        let changed = project(
            &entries,
            |target| parents.get(&target).copied(),
            |_| Vec::new(),
        );
        assert_eq!(
            changed.iter().map(|row| row.target).collect::<Vec<_>>(),
            [terminal, b, a, c, d, nested, a]
        );
        let mut parents = parents;
        parents.insert(a, terminal);
        let changed = project(
            &entries,
            |target| parents.get(&target).copied(),
            |_| Vec::new(),
        );
        assert_eq!(
            changed.iter().map(|row| row.target).collect::<Vec<_>>(),
            [b, terminal, terminal, c, d, nested, a]
        );
    }
}
