use super::*;
use sailry_protocol::{Project, ProjectId, Worktree, WorktreeId, plugin::Action};

fn main_tree() -> Worktree {
    Worktree {
        id: WorktreeId::new(),
        project: Some(ProjectId::new()),
        path: "/project".into(),
        main: true,
    }
}

#[test]
fn invalidates_inventory_without_moving_selection() {
    let main = main_tree();
    let mut state = snapshot(0);
    state.worktrees.push(main.clone());
    let mut projection = Projection::new(state.node, 1);
    projection
        .apply(1, Update::Snapshot(Box::new(state)))
        .unwrap();
    let settled = projection.settled_cursors()[&main.id];
    let actions = [Action::ReadWorktrees];
    let before = projection.read_cursors();
    let linked = Worktree {
        id: WorktreeId::new(),
        project: main.project,
        path: "/linked".into(),
        main: false,
    };
    for (index, event) in [
        Event::WorktreeRegistered(linked.clone()),
        Event::WorktreeRegistered(Worktree {
            path: "/renamed".into(),
            ..linked.clone()
        }),
        Event::WorktreeRemoved { id: linked.id },
    ]
    .into_iter()
    .enumerate()
    {
        let previous = projection.read_cursors().for_actions(&actions);
        let update = Update::Event(EventEnvelope {
            node: NodeId([1; 32]),
            cursor: index as u64 + 1,
            event,
        });
        assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
        let changed = projection.read_cursors().for_actions(&actions);
        assert!(changed > previous);
        assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
        assert_eq!(projection.read_cursors().for_actions(&actions), changed);
        assert_eq!(projection.settled_cursors()[&main.id], settled);
        assert!(projection.snapshot().unwrap().worktrees.contains(&main));
    }
    assert_eq!(
        projection.read_cursors().for_actions(&[Action::ReadModels]),
        before.for_actions(&[Action::ReadModels])
    );
    assert_eq!(projection.snapshot().unwrap().worktrees, [main]);
    let previous = projection.read_cursors();
    projection.apply(1, event(4)).unwrap();
    assert_eq!(projection.read_cursors(), previous);
}

#[test]
fn recovers_project_and_connection_changes() {
    let tree = main_tree();
    let project = Project {
        id: tree.project.unwrap(),
        name: "Project".into(),
        path: tree.path.clone(),
        appearance: Default::default(),
    };
    let mut state = snapshot(0);
    state.projects.push(project.clone());
    state.worktrees.push(tree);
    let mut projection = Projection::new(state.node, 1);
    projection
        .apply(1, Update::Snapshot(Box::new(state.clone())))
        .unwrap();
    let actions = [Action::ReadWorktrees];
    assert!(projection.read_cursors().for_actions(&actions) > 0);
    for (index, event) in [
        Event::ProjectRegistered(project.clone()),
        Event::ProjectChanged(Project {
            name: "Renamed".into(),
            ..project.clone()
        }),
        Event::ProjectRemoved { id: project.id },
    ]
    .into_iter()
    .enumerate()
    {
        let previous = projection.read_cursors().for_actions(&actions);
        state.cursor = index as u64 + 1;
        if matches!(event, Event::ProjectRemoved { .. }) {
            state.projects.clear();
            state.worktrees.clear();
        }
        assert_eq!(
            projection
                .apply(
                    1,
                    Update::Event(EventEnvelope {
                        node: state.node,
                        cursor: state.cursor,
                        event,
                    }),
                )
                .unwrap(),
            Apply::Recover
        );
        projection
            .apply(1, Update::Snapshot(Box::new(state.clone())))
            .unwrap();
        assert!(projection.read_cursors().for_actions(&actions) > previous);
    }
    assert!(projection.snapshot().unwrap().worktrees.is_empty());
    let previous = projection.read_cursors().for_actions(&actions);
    projection.apply(1, Update::ResetRequired).unwrap();
    projection
        .apply(1, Update::Snapshot(Box::new(state.clone())))
        .unwrap();
    assert!(projection.read_cursors().for_actions(&actions) > previous);
    let previous = projection.read_cursors().for_actions(&actions);
    projection.reconnect(2).unwrap();
    assert_eq!(projection.apply(1, event(4)).unwrap(), Apply::Ignored);
    projection
        .apply(2, Update::Snapshot(Box::new(state)))
        .unwrap();
    assert!(projection.read_cursors().for_actions(&actions) > previous);
}
