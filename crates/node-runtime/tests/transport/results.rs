//! Receipts, completions and faults are identical through both Client transports.
use crate::file_fixture::Fixture;
use sailry_client::Client;
use sailry_protocol::*;

/// Receipt durability, absent for a request rejected before admission, and its result.
async fn complete(client: &Client, command: Command) -> (Option<bool>, Result<Output, Fault>) {
    let request = client.prepare(command);
    let admission = match client.dispatch(request.clone()).await {
        Ok(admission) => admission,
        Err(fault) => return (None, Err(fault)),
    };
    assert_eq!(admission.receipt.id, request.id);
    let durable = admission.receipt.durable;
    let result = admission.completion.await.unwrap();
    if durable {
        // A reused durable request identifier reports the original completion.
        assert_eq!(client.execute(request).await, result);
    }
    (Some(durable), result)
}

#[tokio::test]
async fn matches_completions_and_faults() {
    let fixture = Fixture::empty().await;
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    std::fs::write(fixture.root.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(fixture.root.join("large.txt"), "€".repeat(MAX_FILE_BYTES)).unwrap();
    let worktree = fixture.worktree;
    let read = |path: &str| Command::ReadFile {
        worktree,
        path: path.into(),
    };
    let commands = [
        Command::ListDirectory {
            worktree,
            path: String::new(),
            after: None,
        },
        read("src/main.rs"),
        read("large.txt"),
        read("absent.txt"),
        read("../escape.txt"),
        Command::ReadFile {
            worktree: WorktreeId::new(),
            path: "src/main.rs".into(),
        },
        Command::WriteFile {
            worktree,
            path: "src/main.rs".into(),
            text: "changed".into(),
            expected_revision: Some("0".repeat(64)),
        },
    ];
    let [local, remote] = fixture.clients();
    let mut results = Vec::new();
    for command in commands {
        let expected = complete(&local, command.clone()).await;
        assert_eq!(
            complete(&remote, command.clone()).await,
            expected,
            "{command:?}"
        );
        results.push(expected);
    }
    let (Some(false), Ok(Output::Directory(listing))) = &results[0] else {
        panic!("directory expected: {:?}", results[0])
    };
    assert_eq!(listing.entries.len(), 2);
    assert!(!listing.truncated);
    let (Some(false), Ok(Output::FileContent(source))) = &results[1] else {
        panic!("file content expected: {:?}", results[1])
    };
    assert_eq!(source.text, "fn main() {}\n");
    let (Some(false), Ok(Output::FileContent(large))) = &results[2] else {
        panic!("bounded file content expected: {:?}", results[2])
    };
    assert!(large.truncated);
    assert!(large.text.len() <= MAX_FILE_BYTES);
    // Fault details survive the wire unchanged.
    let (None, Err(absent)) = &results[3] else {
        panic!("missing path fault expected: {:?}", results[3])
    };
    assert_eq!(absent.code, ErrorCode::NotFound);
    assert_eq!(absent.details, Some(FaultDetails::WorktreePathNotFound));
    assert!(matches!(
        &results[4],
        (
            None,
            Err(Fault {
                code: ErrorCode::InvalidRequest,
                ..
            })
        )
    ));
    assert!(matches!(
        &results[5],
        (
            None,
            Err(Fault {
                code: ErrorCode::NotFound,
                ..
            })
        )
    ));
    let (Some(true), Err(conflict)) = &results[6] else {
        panic!("durable revision fault expected: {:?}", results[6])
    };
    assert_ne!(conflict.code, ErrorCode::OutcomeUnknown);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    fixture.close().await;
}
