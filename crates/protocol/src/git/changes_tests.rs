use super::*;
use crate::{Command, Output, WorktreeId, tool::Operation};
use serde_json::json;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "encodes_frozen_targets_and_read_contracts",
            (|scenario: &'static str| {
                let snapshot = GitChangeSnapshot {
                    target: GitChangeState::Commit {
                        revision: "a".repeat(40),
                        parent: None,
                    },
                    metadata_revision: "b".repeat(64),
                };
                let cursor = GitChangesCursor {
                    snapshot: snapshot.clone(),
                    after_path: None,
                };
                assert_eq!(
                    serde_json::to_value(&cursor).unwrap(),
                    json!({
                        "snapshot":{"target":{"kind":"commit","data":{"revision":"a".repeat(40),"parent":null}},"metadata_revision":"b".repeat(64)},
                        "after_path":null,
                    }),
                    "{scenario}"
                );
                for command in [
                    Command::ListGitChanges {
                        worktree: WorktreeId::new(),
                        target: None,
                        cursor: Some(cursor),
                        limit: 500,
                    },
                    Command::ReadGitChange {
                        worktree: WorktreeId::new(),
                        snapshot,
                        path: "file.rs".into(),
                        expected_metadata_revision: "c".repeat(64),
                        expected_revision: None,
                        offset: 0,
                        limit: 500,
                    },
                ] {
                    assert!(!command.durable(), "{scenario}");
                    let encoded = serde_json::to_value(&command).unwrap();
                    assert_eq!(
                        serde_json::from_value::<Command>(encoded).unwrap(),
                        command,
                        "{scenario}"
                    );
                }
                assert_eq!(
                    serde_json::to_value(Operation::ListGitChanges).unwrap(),
                    "git.changes",
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::to_value(Operation::ReadGitChange).unwrap(),
                    "git.change",
                    "{scenario}"
                );
                assert!(Operation::ListGitChanges.read_only(), "{scenario}");
                assert!(Operation::ReadGitChange.read_only(), "{scenario}");
                assert_eq!(
                    Operation::ListGitChanges.action(),
                    Some(crate::plugin::Action::ReadGit),
                    "{scenario}"
                );
                assert_eq!(
                    Operation::ReadGitChange.action(),
                    Some(crate::plugin::Action::ReadGit),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "rejects_unknown_snapshot_and_target_fields",
            (|scenario: &'static str| {
                assert!(
                    serde_json::from_value::<GitChangeTarget>(
                        json!({"kind":"worktree","data":{"scope":"all","command":"checkout"}})
                    )
                    .is_err(),
                    "{scenario}"
                );
                assert!(serde_json::from_value::<GitChangesCursor>(json!({"snapshot":{"target":{"kind":"range","data":{"base":"a".repeat(40),"head":"b".repeat(40)}},"metadata_revision":"c".repeat(64)},"after_path":null,"offset":5})).is_err(), "{scenario}");
                let result = Output::GitChanges(GitChanges {
                    snapshot: GitChangeSnapshot {
                        target: GitChangeState::Worktree {
                            scope: GitDiffScope::All,
                            head: None,
                            index_revision: "absent".into(),
                        },
                        metadata_revision: "d".repeat(64),
                    },
                    entries: Vec::new(),
                    next: None,
                    exhausted: false,
                    omitted_paths: 1,
                });
                assert_eq!(
                    serde_json::to_value(&result).unwrap()["kind"],
                    "git_changes",
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Output>(serde_json::to_value(&result).unwrap())
                        .unwrap(),
                    result,
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_exact_context_contract",
            (|scenario: &'static str| {
                let snapshot = GitChangeSnapshot {
                    target: GitChangeState::Commit {
                        revision: "a".repeat(40),
                        parent: None,
                    },
                    metadata_revision: "b".repeat(64),
                };
                for side in [GitContextSide::Old, GitContextSide::New] {
                    let command = Command::ReadGitContext {
                        worktree: WorktreeId::new(),
                        snapshot: snapshot.clone(),
                        path: "file.txt".into(),
                        expected_metadata_revision: "c".repeat(64),
                        expected_revision: "d".repeat(64),
                        side,
                        offset: 0,
                        limit: 100,
                    };
                    assert!(!command.durable(), "{scenario}");
                    let mut value = serde_json::to_value(&command).unwrap();
                    assert_eq!(
                        serde_json::from_value::<Command>(value.clone()).unwrap(),
                        command,
                        "{scenario}"
                    );
                    value["data"]
                        .as_object_mut()
                        .unwrap()
                        .remove("expected_revision");
                    assert!(
                        serde_json::from_value::<Command>(value).is_err(),
                        "{scenario}"
                    );
                    let context = GitContext {
                        snapshot: snapshot.clone(),
                        path: "file.txt".into(),
                        old_path: None,
                        metadata_revision: "c".repeat(64),
                        revision: "d".repeat(64),
                        side,
                        offset: 0,
                        next: None,
                        text: "first\r\nlast".into(),
                        binary: false,
                        skip_reason: None,
                    };
                    let value = serde_json::to_value(&context).unwrap();
                    assert_eq!(
                        serde_json::from_value::<GitContext>(value).unwrap(),
                        context,
                        "{scenario}"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
