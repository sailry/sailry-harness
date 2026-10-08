use super::*;
use crate::{Command, Output, WorktreeId, tool::Operation};
use serde_json::json;

#[test]
fn encodes_frozen_targets_and_read_contracts() {
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
        })
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
        assert!(!command.durable());
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(serde_json::from_value::<Command>(encoded).unwrap(), command);
    }
    assert_eq!(
        serde_json::to_value(Operation::ListGitChanges).unwrap(),
        "git.changes"
    );
    assert_eq!(
        serde_json::to_value(Operation::ReadGitChange).unwrap(),
        "git.change"
    );
    assert!(Operation::ListGitChanges.read_only());
    assert!(Operation::ReadGitChange.read_only());
    assert_eq!(
        Operation::ListGitChanges.action(),
        Some(crate::plugin::Action::ReadGit)
    );
    assert_eq!(
        Operation::ReadGitChange.action(),
        Some(crate::plugin::Action::ReadGit)
    );
}

#[test]
fn rejects_unknown_snapshot_and_target_fields() {
    assert!(
        serde_json::from_value::<GitChangeTarget>(
            json!({"kind":"worktree","data":{"scope":"all","command":"checkout"}})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<GitChangesCursor>(json!({"snapshot":{"target":{"kind":"range","data":{"base":"a".repeat(40),"head":"b".repeat(40)}},"metadata_revision":"c".repeat(64)},"after_path":null,"offset":5})).is_err());
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
        "git_changes"
    );
    assert_eq!(
        serde_json::from_value::<Output>(serde_json::to_value(&result).unwrap()).unwrap(),
        result
    );
}
