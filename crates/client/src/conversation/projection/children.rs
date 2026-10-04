use super::*;
use conversation::Child;
use std::collections::BTreeSet;

pub(in crate::conversation) fn merge(
    page: &mut Page,
    child: Child,
    keep_current: bool,
) -> Result<(), Fault> {
    check(page, &child)?;
    if let Some(previous) = page
        .children
        .iter_mut()
        .find(|entry| entry.run.turn == child.run.turn)
    {
        if previous.origin != child.origin
            || previous.name != child.name
            || previous.run.session != child.run.session
            || previous.run.sequence != child.run.sequence
            || previous.run.revision != child.run.revision
            || previous.run.worktree != child.run.worktree
            || previous.run.origin != child.run.origin
            || closed(previous.run.status) && closed(child.run.status) && previous.run != child.run
            || !keep_current
                && (closed(previous.run.status) && previous.run != child.run
                    || previous.run.status == Status::Stopping
                        && child.run.status == Status::Running)
        {
            return Err(invalid("child identity or terminal state changed"));
        }
        if !keep_current {
            *previous = child;
        }
    } else {
        if page.children.iter().any(|entry| {
            entry.run.session == child.run.session
                || entry.run.sequence == child.run.sequence
                || (entry.origin.turn, &entry.origin.entry, entry.origin.index)
                    == (child.origin.turn, &child.origin.entry, child.origin.index)
        }) {
            return Err(invalid("conversation repeats a child or delegation call"));
        }
        page.children.push(child);
        page.children.sort_by_key(|child| child.run.sequence);
    }
    Ok(())
}

pub(super) fn validate(page: &Page) -> Result<(), Fault> {
    let mut sessions = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut calls = BTreeSet::new();
    if page
        .children
        .windows(2)
        .any(|pair| pair[0].run.sequence >= pair[1].run.sequence)
    {
        return Err(invalid("child admission order changed"));
    }
    for child in &page.children {
        if !sessions.insert(child.run.session)
            || !turns.insert(child.run.turn)
            || !calls.insert((child.origin.turn, &child.origin.entry, child.origin.index))
        {
            return Err(invalid("conversation repeats a child or delegation call"));
        }
        check(page, child)?;
    }
    Ok(())
}

fn check(page: &Page, child: &Child) -> Result<(), Fault> {
    let parent = page
        .runs
        .iter()
        .find(|run| run.turn == child.origin.turn)
        .ok_or_else(|| invalid("child has no parent turn"))?;
    if child.origin.session != parent.origin.unwrap_or(page.session)
        || child.run.session == child.origin.session
        || child.run.session == page.session
        || child.run.sequence <= parent.sequence
        || child.run.revision == 0
        || child.run.kind != conversation::RunKind::Task
        || parent.kind != conversation::RunKind::Task
        || page.runs.iter().any(|run| run.turn == child.run.turn)
        || child.run.origin.is_some()
        || child.run.status == Status::Queued
        || child.origin.role.is_some() != child.name.is_some()
        || child.name.as_ref().is_some_and(|name| name.is_empty())
    {
        return Err(invalid("child does not belong to its parent turn"));
    }
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == child.origin.entry)
        .ok_or_else(|| invalid("child has no canonical delegation call"))?;
    let Some(Part::ToolCall { arguments, .. }) = entry.parts.get(child.origin.index) else {
        return Err(invalid("child references another turn or tool"));
    };
    if entry.turn != parent.turn {
        return Err(invalid("child references another turn or tool"));
    }
    let worktree = arguments
        .get("worktree")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<WorktreeId>(value.clone()))
        .transpose()
        .map_err(|_| invalid("child delegation worktree is invalid"))?
        .unwrap_or(parent.worktree);
    if child.run.worktree != worktree {
        return Err(invalid("child worktree differs from its delegation"));
    }
    Ok(())
}

fn closed(status: Status) -> bool {
    !matches!(status, Status::Queued | Status::Running | Status::Stopping)
}
