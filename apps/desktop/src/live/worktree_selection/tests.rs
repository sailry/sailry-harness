use super::*;
use core::prelude::v1::test;
use sailry_protocol::{Defaults, Project};

fn snapshot() -> Snapshot {
    Snapshot {
        node: NodeId([1; 32]),
        cursor: 1,
        defaults: Defaults {
            revision: 0,
            config: None,
        },
        projects: vec![],
        worktrees: vec![],
        sessions: vec![],
        terminals: vec![],
        terminal_settings_revision: 0,
        power_settings_revision: 0,
        sandbox_settings_revision: 0,
        team_settings_revision: 0,
        compaction_settings_revision: 0,
        search_settings_revision: 0,
        media_settings: Default::default(),
        providers: vec![],
        provider_quotas: vec![],
        model_catalog: Default::default(),
        roles: vec![],
        teams: vec![],
        ssh: vec![],
        databases: vec![],
        plugins: vec![],
        notifications: vec![],
        turns: vec![],
    }
}

fn register(snapshot: &mut Snapshot) -> (ProjectId, WorktreeId) {
    let project = ProjectId::new();
    let tree = WorktreeId::new();
    snapshot.projects.push(Project {
        id: project,
        name: "Project".into(),
        path: "/project".into(),
        appearance: Default::default(),
    });
    snapshot.worktrees.push(Worktree {
        id: tree,
        project: Some(project),
        path: "/project".into(),
        main: true,
    });
    (project, tree)
}

#[test]
fn removes_stale_choices() {
    let mut before = snapshot();
    let (removed, removed_tree) = register(&mut before);
    let (retained, retained_tree) = register(&mut before);
    let other_node = NodeId([2; 32]);
    let mut choices = BTreeMap::from([
        ((before.node, removed), removed_tree),
        ((before.node, retained), retained_tree),
        ((other_node, removed), removed_tree),
    ]);
    let mut after = before.clone();
    after.cursor += 1;
    after.projects.retain(|project| project.id != removed);

    assert!(project_removed(removed, &after, Some(&before)));
    assert!(project_removed(removed, &after, Some(&after)));
    assert!(!project_removed(retained, &after, Some(&before)));
    retain_choices(&mut choices, &after, Some(&before));
    assert_eq!(
        choices,
        BTreeMap::from([
            ((before.node, retained), retained_tree),
            ((other_node, removed), removed_tree),
        ])
    );
    assert_eq!(after.worktrees, before.worktrees);
}

#[test]
fn preserves_response_before_snapshot() {
    let before = snapshot();
    let mut after = before.clone();
    let (project, tree) = register(&mut after);
    let mut choices = BTreeMap::from([((before.node, project), tree)]);

    assert!(!project_removed(project, &before, None));
    retain_choices(&mut choices, &before, None);
    assert_eq!(choices.get(&(before.node, project)), Some(&tree));
    assert!(!project_removed(project, &before, Some(&before)));
    retain_choices(&mut choices, &before, Some(&before));
    assert_eq!(choices.get(&(before.node, project)), Some(&tree));
    retain_choices(&mut choices, &after, Some(&before));
    assert_eq!(choices.get(&(before.node, project)), Some(&tree));
    assert!(!project_removed(project, &after, Some(&before)));
}

#[test]
fn refuses_removed_draft_scope() {
    let mut snapshot = snapshot();
    let (project, tree) = register(&mut snapshot);
    assert_eq!(
        restored_scope(Some(project), Some(tree), Some(&snapshot)),
        (Some(project), None)
    );
    snapshot.projects.clear();
    assert_eq!(
        restored_scope(Some(project), Some(tree), Some(&snapshot)),
        (None, None)
    );
    assert_eq!(
        restored_scope(Some(project), Some(tree), None),
        (Some(project), None)
    );
    let unassigned = WorktreeId::new();
    snapshot.worktrees.push(Worktree {
        id: unassigned,
        project: None,
        path: "/unassigned".into(),
        main: true,
    });
    assert_eq!(
        restored_scope(None, Some(unassigned), Some(&snapshot)),
        (None, Some(unassigned))
    );
}

#[test]
fn recovers_missing_worktree() {
    let mut before = snapshot();
    let (project, tree) = register(&mut before);
    let mut after = before.clone();
    after.worktrees.clear();
    let mut choices = BTreeMap::from([((before.node, project), tree)]);
    retain_choices(&mut choices, &after, Some(&before));
    assert!(choices.is_empty());
    assert!(!project_removed(project, &after, Some(&before)));
    after.projects.clear();
    assert!(project_removed(project, &after, Some(&before)));
    assert!(!project_removed(project, &after, None));
}
