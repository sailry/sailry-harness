use super::*;

#[test]
fn owned_resources() {
    for (scenario, check) in [
        (
            "projects_are_not_host_indices",
            (|_scenario: &str| {
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
                assert!(state.select(owner), "projects are not host indices");
                let session = state.create_session(owner);
                let terminal = state.create_terminal(owner);
                assert_eq!(
                    state.sessions[&session].owner, owner,
                    "projects are not host indices"
                );
                assert_eq!(
                    state.terminals[&terminal].owner, owner,
                    "projects are not host indices"
                );
                assert_eq!(state.owner(0), owner, "projects are not host indices");
                assert_eq!(state.owner(1).project, 1, "projects are not host indices");
                assert!(
                    !state.select(Owner { host: 1, ..owner }),
                    "projects are not host indices"
                );
                assert!(
                    !state.select(Owner {
                        worktree: 0,
                        ..owner
                    }),
                    "projects are not host indices"
                );
                assert_eq!(state.owner(0), owner, "projects are not host indices");
            }) as fn(&str),
        ),
        (
            "closed_ids_are_not_reused",
            (|_scenario: &str| {
                let mut state = State::default();
                let owner = state.owner(0);
                let first = state.create_terminal(owner);
                state.terminals.remove(&first);
                let second = state.create_terminal(owner);
                assert!(second.1 > first.1, "closed ids are not reused");
                let first = state.create_session(owner);
                state.sessions.remove(&first);
                let second = state.create_session(owner);
                assert!(second.1 > first.1, "closed ids are not reused");
            }),
        ),
        (
            "selection_preserves_owner",
            (|_scenario: &str| {
                let mut state = State::default();
                state.ensure_session((0, 2));
                let original = state.sessions[&(0, 2)].owner;
                assert!(
                    state.select(Owner {
                        worktree: 2,
                        ..original
                    }),
                    "selection preserves owner"
                );
                state.ensure_session((0, 2));
                assert_eq!(
                    state.sessions[&(0, 2)].owner,
                    original,
                    "selection preserves owner"
                );
                let key = state.create_session(state.owner(0));
                assert_eq!(
                    state.sessions[&key].owner.worktree, 2,
                    "selection preserves owner"
                );
            }),
        ),
        (
            "worktree_validation",
            (|_scenario: &str| {
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
                    assert_eq!(
                        state.add_worktree(project, base, branch, path),
                        Err(error),
                        "worktree validation"
                    );
                    assert_eq!(state.worktrees.len(), 4, "worktree validation");
                }
                let local = state
                    .add_worktree(0, " main ", " feature ", " /preview/new ")
                    .unwrap();
                let remote = state
                    .add_worktree(1, "main", "feature", "/preview/new")
                    .unwrap();
                assert_ne!(local.worktree, remote.worktree, "worktree validation");
                assert_eq!(
                    state.worktrees[&local.worktree].branch, "feature",
                    "worktree validation"
                );
                assert_eq!(
                    state.worktrees[&local.worktree].base_ref.as_deref(),
                    Some("main"),
                    "worktree validation"
                );
                assert_eq!(state.owner(0), initial, "worktree validation");
                state.worktrees.remove(&remote.worktree);
                assert!(
                    state
                        .add_worktree(1, "main", "feature", "/preview/new")
                        .unwrap()
                        .worktree
                        > remote.worktree,
                    "worktree validation"
                );
            }),
        ),
        (
            "project_paths_are_host_scoped",
            (|_scenario: &str| {
                let mut state = State::default();
                assert_eq!(
                    state.save_project(0, None, " ", "/preview/new"),
                    Err("project_required"),
                    "project paths are host scoped"
                );
                assert_eq!(
                    state.save_project(0, None, "Duplicate", "/preview/sailry/"),
                    Err("project_duplicate_path"),
                    "project paths are host scoped"
                );
                let local = state
                    .save_project(0, None, " New ", " /preview/new ")
                    .unwrap();
                let remote = state
                    .save_project(1, None, "Remote", "/preview/new")
                    .unwrap();
                assert_ne!(local, remote, "project paths are host scoped");
                assert!(
                    !state.projects[&local].trusted,
                    "project paths are host scoped"
                );
                assert_eq!(
                    state.projects[&local].name, "New",
                    "project paths are host scoped"
                );
                let before = state.projects[&local].clone();
                let old_tree = before.active_worktree;
                state
                    .save_project(0, Some((local, &before)), "Moved", "/preview/moved")
                    .unwrap();
                assert!(
                    !state.worktrees.contains_key(&old_tree),
                    "project paths are host scoped"
                );
                assert!(
                    state.projects[&local].active_worktree > old_tree,
                    "project paths are host scoped"
                );
                assert_eq!(
                    state.projects[&remote].path, "/preview/new",
                    "project paths are host scoped"
                );
                assert_eq!(
                    state.save_project(0, Some((local, &before)), "Stale", "/preview/other"),
                    Err("project_changed"),
                    "project paths are host scoped"
                );
            }),
        ),
        (
            "preserves_referenced_paths",
            (|_scenario: &str| {
                let mut state = State::default();
                let original = state.projects[&0].clone();
                assert_eq!(
                    state.save_project(0, Some((0, &original)), "New", "/preview/other"),
                    Err("project_path_trusted"),
                    "preserves referenced paths"
                );
                state.projects.get_mut(&0).unwrap().trusted = false;
                for session in state.sessions.values_mut() {
                    session.archived = true;
                }
                assert_eq!(
                    state.save_project(0, Some((0, &original)), "New", "/preview/other"),
                    Err("project_path_referenced"),
                    "preserves referenced paths"
                );
                assert_eq!(
                    state.projects[&0].name, original.name,
                    "preserves referenced paths"
                );
                assert_eq!(
                    state.projects[&0].path, original.path,
                    "preserves referenced paths"
                );
                state
                    .save_project(0, Some((0, &original)), "Renamed", &original.path)
                    .unwrap();
                assert_eq!(
                    state.worktrees[&0].path, original.path,
                    "preserves referenced paths"
                );
                assert_eq!(
                    state.sessions[&(0, 0)].owner.worktree,
                    0,
                    "preserves referenced paths"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
