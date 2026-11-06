use super::*;
use core::prelude::v1::test;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Defaults, Fault, ProjectId, Snapshot, Topic, Worktree};

struct Offline(NodeId);

impl Transport for Offline {
    fn target(&self) -> NodeId {
        self.0
    }

    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("selection checks must not dispatch requests")
    }

    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        panic!("selection checks must not create subscriptions")
    }
}

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

fn binding(snapshot: &mut Snapshot) -> Binding {
    let project = ProjectId::new();
    let tree = WorktreeId::new();
    snapshot.projects.push(Project {
        id: project,
        name: "Original".into(),
        path: "/project".into(),
        appearance: Default::default(),
    });
    snapshot.worktrees.push(Worktree {
        id: tree,
        project: Some(project),
        path: "/project".into(),
        main: true,
    });
    let client = Arc::new(Client::new(Arc::new(Offline(snapshot.node))));
    Binding {
        client: client.clone(),
        defaults: client,
        runtime: Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap(),
        ),
        project: Some(project),
        worktree: Some(tree),
        host: "Fixture Node".into(),
        project_name: "Original".into(),
        branch: "main".into(),
    }
}

#[test]
fn clears_removed_scope() {
    let mut snapshot = snapshot();
    let mut binding = binding(&mut snapshot);
    let client = binding.client.clone();
    let defaults = binding.defaults.clone();
    let runtime = binding.runtime.clone();
    let old_tree = binding.worktree;
    snapshot.projects.clear();

    assert!(binding.refresh_project(&snapshot));
    assert_eq!(binding.project, None);
    assert_eq!(binding.worktree, None);
    assert!(binding.project_name.is_empty());
    assert!(binding.branch.is_empty());
    assert_eq!(binding.host.as_ref(), "Fixture Node");
    assert!(Arc::ptr_eq(&binding.client, &client));
    assert!(Arc::ptr_eq(&binding.defaults, &defaults));
    assert!(Arc::ptr_eq(&binding.runtime, &runtime));
    assert_eq!(Some(snapshot.worktrees[0].id), old_tree);
    assert!(!binding.refresh_project(&snapshot));
}

#[test]
fn refreshes_name_without_rebinding() {
    let mut snapshot = snapshot();
    let mut binding = binding(&mut snapshot);
    let scope = (binding.project, binding.worktree, binding.branch.clone());
    snapshot.projects[0].name = "Renamed".into();

    assert!(!binding.refresh_project(&snapshot));
    assert_eq!(binding.project_name.as_ref(), "Renamed");
    assert_eq!(
        (binding.project, binding.worktree, binding.branch.clone()),
        scope
    );
    snapshot.node = NodeId([2; 32]);
    snapshot.projects.clear();
    assert!(!binding.refresh_project(&snapshot));
    assert_eq!((binding.project, binding.worktree, binding.branch), scope);
}

#[test]
fn blocks_removed_submission() {
    let mut snapshot = snapshot();
    let mut binding = binding(&mut snapshot);
    assert!(binding.project_available(Some(&snapshot)));
    assert!(!binding.project_available(None));
    snapshot.projects.clear();
    assert!(!binding.project_available(Some(&snapshot)));
    assert!(binding.refresh_project(&snapshot));
    assert!(binding.project_available(Some(&snapshot)));
}

#[test]
fn rejects_stale_picker_entries() {
    let mut snapshot = snapshot();
    let binding = binding(&mut snapshot);
    let project = binding.project.unwrap();
    let (current, tree) = registered_project(&snapshot, project).unwrap();
    assert_eq!(current.id, project);
    assert_eq!(Some(tree.id), binding.worktree);

    snapshot.projects.clear();
    assert!(registered_project(&snapshot, project).is_none());
    assert_eq!(snapshot.worktrees[0].project, Some(project));
}
