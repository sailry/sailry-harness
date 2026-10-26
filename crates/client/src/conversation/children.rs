//! Child selection and registered session ownership shared by controllers.
use sailry_protocol::{
    ProjectId, RoleId, Session, SessionId, Snapshot, TurnId, WorktreeId,
    conversation::{Child, Page, Status},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub index: usize,
    pub target: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct View {
    pub sessions: BTreeMap<SessionId, Reference>,
}

pub fn active(status: Status) -> bool {
    matches!(status, Status::Running | Status::Stopping)
}

pub fn rank(child: &Child) -> (bool, bool, u64) {
    (
        active(child.run.status),
        child.run.status != Status::Queued,
        child.run.sequence,
    )
}

pub fn latest<'a>(children: impl IntoIterator<Item = &'a Child>) -> Vec<&'a Child> {
    let mut selected = BTreeMap::<SessionId, &Child>::new();
    for child in children
        .into_iter()
        .filter(|child| child.run.joined.is_none())
    {
        let current = selected.entry(child.run.session).or_insert(child);
        if rank(child) > rank(current) {
            *current = child;
        }
    }
    let mut children: Vec<_> = selected.into_values().collect();
    children.sort_by_key(|child| child.run.sequence);
    children
}

pub fn running(children: &[Child], parent: Option<SessionId>) -> Vec<&Child> {
    latest(
        children
            .iter()
            .filter(|child| Some(child.origin.session) == parent),
    )
    .into_iter()
    .filter(|child| child.run.status == Status::Running)
    .collect()
}

pub fn selected(children: &[Child], session: SessionId) -> Option<&Child> {
    children
        .iter()
        .filter(|child| child.run.session == session && child.run.joined.is_none())
        .max_by_key(|child| rank(child))
}

pub fn associated(page: &Page, turn: TurnId, entry: &str, index: usize) -> Vec<Reference> {
    latest(page.children.iter().filter(|child| {
        child.origin.session == page.session
            && child.origin.turn == turn
            && child.origin.entry == entry
            && child.origin.index == index
    }))
    .into_iter()
    .map(|child| Reference {
        index: page
            .children
            .iter()
            .position(|entry| std::ptr::eq(entry, child))
            .unwrap(),
        target: target_key(child),
    })
    .collect()
}

pub fn project(page: &Page) -> View {
    View {
        sessions: latest(&page.children)
            .into_iter()
            .map(|child| {
                (
                    child.run.session,
                    Reference {
                        index: page
                            .children
                            .iter()
                            .position(|entry| std::ptr::eq(entry, child))
                            .unwrap(),
                        target: target_key(child),
                    },
                )
            })
            .collect(),
    }
}

pub fn key(child: &Child) -> String {
    format!(
        "{}-{}-{}-{}",
        child.run.session, child.origin.turn, child.origin.entry, child.origin.index
    )
}

pub fn target_key(child: &Child) -> String {
    target(
        child.run.session,
        child.run.worktree,
        child.origin.session,
        child.origin.role,
    )
}

fn target(
    session: SessionId,
    worktree: WorktreeId,
    parent: SessionId,
    role: Option<RoleId>,
) -> String {
    serde_json::to_string(&(session, worktree, parent, role)).expect("child identities serialize")
}

pub fn session<'a>(
    snapshot: &'a Snapshot,
    project: Option<ProjectId>,
    child: &Child,
) -> Option<&'a Session> {
    let parent = snapshot
        .sessions
        .iter()
        .find(|session| session.id == child.origin.session)?;
    if parent.project != project {
        return None;
    }
    snapshot.sessions.iter().find(|session| {
        session.id == child.run.session
            && session.project == project
            && session.worktree == child.run.worktree
            && registered(snapshot, session)
            && session.delegation.as_ref().is_some_and(|origin| {
                origin.session == child.origin.session && origin.role == child.origin.role
            })
    })
}

fn registered(snapshot: &Snapshot, session: &Session) -> bool {
    snapshot
        .worktrees
        .iter()
        .any(|tree| tree.id == session.worktree && tree.project == session.project)
}

pub fn targets(snapshot: &Snapshot) -> BTreeMap<String, SessionId> {
    snapshot
        .sessions
        .iter()
        .filter_map(|session| {
            let origin = session.delegation.as_ref()?;
            let parent = snapshot
                .sessions
                .iter()
                .find(|parent| parent.id == origin.session)?;
            (session.project == parent.project && registered(snapshot, session)).then(|| {
                (
                    target(session.id, session.worktree, origin.session, origin.role),
                    session.id,
                )
            })
        })
        .collect()
}
