//! Semantic turn changes shared by controller presentation caches.
use sailry_protocol::{TurnId, conversation::Snapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// `None` invalidates all turns after a structural history replacement.
pub fn changed(previous: &Snapshot, next: &Snapshot) -> Option<BTreeSet<TurnId>> {
    if previous.node != next.node
        || previous.page.session != next.page.session
        || previous.page.revision != next.page.revision
    {
        return None;
    }
    let mut changed = BTreeSet::new();
    collect(&mut changed, &previous.drafts, &next.drafts, |value| {
        value.turn
    });
    if !Arc::ptr_eq(&previous.page, &next.page) {
        let before = &previous.page;
        let after = &next.page;
        collect(&mut changed, &before.runs, &after.runs, |value| value.turn);
        collect(
            &mut changed,
            &before.queue.items,
            &after.queue.items,
            |value| value.turn,
        );
        collect(&mut changed, &before.approvals, &after.approvals, |value| {
            value.turn
        });
        collect(&mut changed, &before.questions, &after.questions, |value| {
            value.turn
        });
        collect(&mut changed, &before.children, &after.children, |value| {
            value.origin.turn
        });
        collect(
            &mut changed,
            &before.background,
            &after.background,
            |value| value.turn,
        );
        if !Arc::ptr_eq(&before.entries, &after.entries) {
            // Canonical entry identities and sequences are immutable within a revision.
            let before: Vec<_> = before
                .entries
                .iter()
                .map(|entry| (entry.turn, &entry.id, entry.sequence))
                .collect();
            let after: Vec<_> = after
                .entries
                .iter()
                .map(|entry| (entry.turn, &entry.id, entry.sequence))
                .collect();
            collect(&mut changed, &before, &after, |value| value.0);
        }
        // Joined guidance renders in its response owner's row.
        for run in before.runs.iter().chain(&after.runs) {
            if changed.contains(&run.turn)
                && let Some(owner) = run.joined
            {
                changed.insert(owner);
            }
        }
    }
    Some(changed)
}

fn collect<T: PartialEq>(
    changed: &mut BTreeSet<TurnId>,
    before: &[T],
    after: &[T],
    turn: impl Fn(&T) -> TurnId,
) {
    let group = |values: &[T]| {
        let mut groups = BTreeMap::<TurnId, Vec<usize>>::new();
        for (index, value) in values.iter().enumerate() {
            groups.entry(turn(value)).or_default().push(index);
        }
        groups
    };
    let left = group(before);
    let right = group(after);
    for key in left.keys().chain(right.keys()) {
        let equal = match (left.get(key), right.get(key)) {
            (Some(a), Some(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| before[*a] == after[*b])
            }
            _ => false,
        };
        if !equal {
            changed.insert(*key);
        }
    }
}
