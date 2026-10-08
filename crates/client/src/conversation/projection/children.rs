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
            return Err(sailry_protocol::Fault::invalid(
                "child identity or terminal state changed",
            ));
        }
        if !keep_current {
            *previous = child;
        }
    } else {
        if page
            .children
            .iter()
            .any(|entry| entry.run.sequence == child.run.sequence)
        {
            return Err(sailry_protocol::Fault::invalid(
                "child admission order repeats",
            ));
        }
        page.children.push(child);
        page.children.sort_by_key(|child| child.run.sequence);
    }
    Ok(())
}

pub(super) fn validate(page: &Page) -> Result<(), Fault> {
    let mut turns = BTreeSet::new();
    if page
        .children
        .windows(2)
        .any(|pair| pair[0].run.sequence >= pair[1].run.sequence)
    {
        return Err(sailry_protocol::Fault::invalid(
            "child admission order changed",
        ));
    }
    for child in &page.children {
        if !turns.insert(child.run.turn) {
            return Err(sailry_protocol::Fault::invalid("child turn repeats"));
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
        .ok_or_else(|| sailry_protocol::Fault::invalid("child has no parent turn"))?;
    if child.origin.session != parent.origin.unwrap_or(page.session)
        || child.run.session == child.origin.session
        || child.run.session == page.session
        || child.run.sequence <= parent.sequence
        || child.run.revision == 0
        || child.run.kind != conversation::RunKind::Task
        || parent.kind != conversation::RunKind::Task
        || page.runs.iter().any(|run| run.turn == child.run.turn)
        || child.run.origin.is_some()
        || child.origin.role.is_some() != child.name.is_some()
        || child.name.as_ref().is_some_and(|name| name.is_empty())
    {
        return Err(sailry_protocol::Fault::invalid(
            "child does not belong to its parent turn",
        ));
    }
    for previous in &page.children {
        if previous.run.session == child.run.session
            && (previous.origin.session != child.origin.session
                || previous.origin.role != child.origin.role
                || previous.name != child.name
                || previous.run.revision != child.run.revision
                || previous.run.worktree != child.run.worktree)
        {
            return Err(sailry_protocol::Fault::invalid(
                "child ownership or frozen configuration changed",
            ));
        }
        if (
            previous.origin.turn,
            &previous.origin.entry,
            previous.origin.index,
        ) == (child.origin.turn, &child.origin.entry, child.origin.index)
            && previous.run.session != child.run.session
        {
            return Err(sailry_protocol::Fault::invalid(
                "delegation call belongs to another child",
            ));
        }
    }
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == child.origin.entry)
        .ok_or_else(|| sailry_protocol::Fault::invalid("child has no canonical delegation call"))?;
    let Some(Part::ToolCall { arguments, .. }) = entry.parts.get(child.origin.index) else {
        return Err(sailry_protocol::Fault::invalid(
            "child references another turn or tool",
        ));
    };
    if entry.turn != parent.turn {
        return Err(sailry_protocol::Fault::invalid(
            "child references another turn or tool",
        ));
    }
    let session = arguments
        .get("session")
        .map(|value| serde_json::from_value::<SessionId>(value.clone()))
        .transpose()
        .map_err(|_| sailry_protocol::Fault::invalid("child delegation session is invalid"))?;
    if session.is_some_and(|session| session != child.run.session) {
        return Err(sailry_protocol::Fault::invalid(
            "child session differs from its delegation",
        ));
    }
    let worktree = arguments
        .get("worktree")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<WorktreeId>(value.clone()))
        .transpose()
        .map_err(|_| sailry_protocol::Fault::invalid("child delegation worktree is invalid"))?
        .unwrap_or_else(|| {
            if session.is_some() {
                child.run.worktree
            } else {
                parent.worktree
            }
        });
    if child.run.worktree != worktree {
        return Err(sailry_protocol::Fault::invalid(
            "child worktree differs from its delegation",
        ));
    }
    Ok(())
}

fn closed(status: Status) -> bool {
    !matches!(status, Status::Queued | Status::Running | Status::Stopping)
}
