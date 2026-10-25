use super::*;
use sailry_protocol::{Command, FileLines, WorktreeId, code_index};

#[test]
fn preserves_query_and_source_revisions() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let worktree = WorktreeId::new();
    let mut scope = context();
    scope.worktree = Some(worktree);
    let result = run(
        bundle(
            r#"
            import {searchCode,outlineCode,readFileLines} from 'sailry/sdk';
            export async function run() {
                const search = await searchCode({name:'entry',limit:2});
                const outline = await outlineCode('main.rs');
                const lines = await readFileLines('main.rs',1,3,outline.file_revision);
                return {search,outline,lines};
            }
        "#,
        ),
        "run",
        json!(null),
        scope.clone(),
        NodeId([1; 32]),
        CancellationToken::new(),
        move |request| {
            captured.lock().unwrap().push(request.clone());
            match request.command {
                Command::SearchCode {
                    query,
                    worktree: actual,
                } => {
                    assert_eq!(actual, worktree);
                    assert_eq!(query.name.as_deref(), Some("entry"));
                    assert_eq!(query.limit, 2);
                    Ok(Output::CodeSearch(code_index::SearchPage {
                        symbols: Vec::new(),
                        index_revision: 4,
                        partial: false,
                        truncated: false,
                        visited: 1,
                        skipped: 0,
                        issues: Vec::new(),
                    }))
                }
                Command::OutlineCode {
                    path,
                    worktree: actual,
                } => {
                    assert_eq!(actual, worktree);
                    Ok(Output::CodeOutline(code_index::Outline {
                        path,
                        language: Some("rust".into()),
                        file_revision: Some("f".repeat(64)),
                        index_revision: 4,
                        status: code_index::Status::Parsed,
                        symbols: Vec::new(),
                        capabilities: Default::default(),
                        partial: false,
                        truncated: false,
                    }))
                }
                Command::ReadFileLines {
                    path,
                    worktree: actual,
                    offset,
                    limit,
                    expected_revision,
                } => {
                    assert_eq!(actual, worktree);
                    assert_eq!(offset, 1);
                    assert_eq!(limit, 3);
                    assert_eq!(expected_revision, Some("f".repeat(64)));
                    Ok(Output::FileLines(FileLines {
                        path,
                        lines: Vec::new(),
                        size: 0,
                        revision: "f".repeat(64),
                        offset,
                        next_offset: None,
                    }))
                }
                _ => panic!("code query or source read expected"),
            }
        },
    )
    .unwrap();
    assert_eq!(result["search"]["index_revision"], 4);
    assert_eq!(result["lines"]["revision"], "f".repeat(64));
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.plugin.as_ref() == Some(&scope))
    );
}
