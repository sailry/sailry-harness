use super::*;

#[test]
fn projects_are_not_host_indices() {
    let mut state = State::default();
    state.projects.insert(
        42,
        Project {
            host: 0,
            name: "Second project".into(),
            path: "/preview/second".into(),
            active_worktree: 99,
            trusted: true,
            repository: Repository::Ready,
            appearance: Default::default(),
        },
    );
    state.worktrees.insert(
        99,
        Worktree {
            project: 42,
            branch: "feature/second".into(),
            path: "/preview/second/worktree".into(),
            base_ref: None,
            main: true,
            locked: false,
            dirty: false,
            busy: false,
        },
    );
    let owner = Owner {
        host: 0,
        project: 42,
        worktree: 99,
    };
    assert!(state.select(owner));
    let session = state.create_session(owner);
    let terminal = state.create_terminal(owner);
    assert_eq!(state.sessions[&session].owner, owner);
    assert_eq!(state.terminals[&terminal].owner, owner);
    assert_eq!(state.owner(0), owner);
    assert_eq!(state.owner(1).project, 1);
    assert!(!state.select(Owner { host: 1, ..owner }));
    assert!(!state.select(Owner {
        worktree: 0,
        ..owner
    }));
    assert_eq!(state.owner(0), owner);
}

#[test]
fn closed_ids_are_not_reused() {
    let mut state = State::default();
    let owner = state.owner(0);
    let first = state.create_terminal(owner);
    state.terminals.remove(&first);
    let second = state.create_terminal(owner);
    assert!(second.1 > first.1);
    let first = state.create_session(owner);
    state.sessions.remove(&first);
    let second = state.create_session(owner);
    assert!(second.1 > first.1);
}

#[test]
fn selection_preserves_owner() {
    let mut state = State::default();
    state.ensure_session((0, 2));
    let original = state.sessions[&(0, 2)].owner;
    assert!(state.select(Owner {
        worktree: 2,
        ..original
    }));
    state.ensure_session((0, 2));
    assert_eq!(state.sessions[&(0, 2)].owner, original);
    let key = state.create_session(state.owner(0));
    assert_eq!(state.sessions[&key].owner.worktree, 2);
}

#[test]
fn worktree_validation() {
    let mut state = State::default();
    let initial = state.owner(0);
    for (project, base, branch, path, error) in [
        (
            99,
            "main",
            "feature",
            "/preview/new",
            "worktree_missing_project",
        ),
        (0, " ", "feature", "/preview/new", "worktree_required"),
        (0, "main", "", "/preview/new", "worktree_required"),
        (0, "main", "feature", " ", "worktree_required"),
        (
            0,
            "main",
            "main",
            "/preview/new",
            "worktree_duplicate_branch",
        ),
        (
            0,
            "main",
            "feature",
            "/preview/sailry/",
            "worktree_duplicate_path",
        ),
    ] {
        assert_eq!(state.add_worktree(project, base, branch, path), Err(error));
        assert_eq!(state.worktrees.len(), 4);
    }
    let local = state
        .add_worktree(0, " main ", " feature ", " /preview/new ")
        .unwrap();
    let remote = state
        .add_worktree(1, "main", "feature", "/preview/new")
        .unwrap();
    assert_ne!(local.worktree, remote.worktree);
    assert_eq!(state.worktrees[&local.worktree].branch, "feature");
    assert_eq!(
        state.worktrees[&local.worktree].base_ref.as_deref(),
        Some("main")
    );
    assert_eq!(state.owner(0), initial);
    state.worktrees.remove(&remote.worktree);
    assert!(
        state
            .add_worktree(1, "main", "feature", "/preview/new")
            .unwrap()
            .worktree
            > remote.worktree
    );
}

#[test]
fn project_paths_are_host_scoped() {
    let mut state = State::default();
    assert_eq!(
        state.save_project(0, None, " ", "/preview/new"),
        Err("project_required")
    );
    assert_eq!(
        state.save_project(0, None, "Duplicate", "/preview/sailry/"),
        Err("project_duplicate_path")
    );
    let local = state
        .save_project(0, None, " New ", " /preview/new ")
        .unwrap();
    let remote = state
        .save_project(1, None, "Remote", "/preview/new")
        .unwrap();
    assert_ne!(local, remote);
    assert!(!state.projects[&local].trusted);
    assert_eq!(state.projects[&local].name, "New");
    let before = state.projects[&local].clone();
    let old_tree = before.active_worktree;
    state
        .save_project(0, Some((local, &before)), "Moved", "/preview/moved")
        .unwrap();
    assert!(!state.worktrees.contains_key(&old_tree));
    assert!(state.projects[&local].active_worktree > old_tree);
    assert_eq!(state.projects[&remote].path, "/preview/new");
    assert_eq!(
        state.save_project(0, Some((local, &before)), "Stale", "/preview/other"),
        Err("project_changed")
    );
}

#[test]
fn preserves_referenced_paths() {
    let mut state = State::default();
    let original = state.projects[&0].clone();
    assert_eq!(
        state.save_project(0, Some((0, &original)), "New", "/preview/other"),
        Err("project_path_trusted")
    );
    state.projects.get_mut(&0).unwrap().trusted = false;
    for session in state.sessions.values_mut() {
        session.archived = true;
    }
    assert_eq!(
        state.save_project(0, Some((0, &original)), "New", "/preview/other"),
        Err("project_path_referenced")
    );
    assert_eq!(state.projects[&0].name, original.name);
    assert_eq!(state.projects[&0].path, original.path);
    state
        .save_project(0, Some((0, &original)), "Renamed", &original.path)
        .unwrap();
    assert_eq!(state.worktrees[&0].path, original.path);
    assert_eq!(state.sessions[&(0, 0)].owner.worktree, 0);
}
