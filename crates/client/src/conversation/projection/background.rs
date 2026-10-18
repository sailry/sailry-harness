//! Completion records follow authoritative revisions, including recovery to pending.
use sailry_protocol::{
    Fault, TurnId,
    conversation::{
        Page,
        background::{Kind, Task},
    },
};
use std::collections::BTreeSet;

pub(super) fn validate(page: &Page) -> Result<(), Fault> {
    let mut ids = BTreeSet::new();
    for task in &page.background {
        check(page, task)?;
        if !ids.insert(&task.id) {
            return Err(Fault::invalid("background task identity is repeated"));
        }
    }
    Ok(())
}

fn check(page: &Page, task: &Task) -> Result<(), Fault> {
    let identity = match &task.kind {
        Kind::Agent { session, .. } => {
            *session != task.session
                && task
                    .id
                    .strip_prefix("agent:")
                    .is_some_and(|id| id.parse::<TurnId>().is_ok())
        }
        Kind::Command { id } => task.id == format!("command:{id}"),
    };
    if !identity
        || task.session != page.session
        || task.revision == 0
        || task.delivery.is_some_and(|delivery| {
            page.runs
                .iter()
                .any(|run| run.turn == delivery && run.session != task.session)
        })
    {
        return Err(Fault::invalid("background task belongs to another scope"));
    }
    Ok(())
}

fn same_source(before: &Task, after: &Task) -> bool {
    before.session == after.session
        && before.turn == after.turn
        && before.started_ms == after.started_ms
        && match (&before.kind, &after.kind) {
            (
                Kind::Agent {
                    session: before, ..
                },
                Kind::Agent { session: after, .. },
            ) => before == after,
            (Kind::Command { id: before }, Kind::Command { id: after }) => before == after,
            _ => false,
        }
}

pub(in crate::conversation) fn merge(page: &mut Page, task: Task) -> Result<(), Fault> {
    check(page, &task)?;
    if let Some(previous) = page
        .background
        .iter_mut()
        .find(|previous| previous.id == task.id)
    {
        if !same_source(previous, &task) {
            return Err(Fault::invalid("background task source changed"));
        }
        if task.revision < previous.revision {
            return Ok(());
        }
        if task.revision == previous.revision {
            if *previous != task {
                return Err(Fault::invalid(
                    "background task changed without its revision",
                ));
            }
            return Ok(());
        }
        *previous = task;
    } else {
        page.background.push(task);
    }
    Ok(())
}
