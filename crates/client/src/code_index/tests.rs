use super::*;
use sailry_link::{Admission, Pending as Future, Subscription};
use sailry_protocol::{ErrorCode, FileLines, NodeId, Receipt, Topic, plugin};
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

struct Backend {
    calls: Mutex<Vec<Request>>,
    outputs: Mutex<VecDeque<Output>>,
    block: AtomicBool,
    entered: Semaphore,
    cancelled: Semaphore,
}

impl Backend {
    fn new(outputs: Vec<Output>) -> Arc<Self> {
        Arc::new(Self {
            calls: Default::default(),
            outputs: Mutex::new(outputs.into()),
            block: AtomicBool::new(false),
            entered: Semaphore::new(0),
            cancelled: Semaphore::new(0),
        })
    }
}

impl Transport for Backend {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }

    fn dispatch(&self, request: Request) -> Future<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(request.clone());
            let output = match request.command {
                Command::CancelCodeIndexQuery { request } => {
                    self.cancelled.add_permits(1);
                    Output::CodeIndexQueryCancelled { request }
                }
                _ => {
                    self.entered.add_permits(1);
                    if self.block.load(Ordering::Relaxed) {
                        std::future::pending::<()>().await;
                    }
                    self.outputs.lock().unwrap().pop_front().unwrap()
                }
            };
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(Ok(output)).unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }

    fn subscribe(&self, _: Topic) -> Future<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(Fault::invalid("unused subscription")) })
    }
}

#[tokio::test]
async fn preserves_shared_command_contracts() {
    let worktree = WorktreeId::new();
    let page = code_index::SearchPage {
        symbols: Vec::new(),
        index_revision: 5,
        partial: true,
        truncated: false,
        visited: 1,
        skipped: 1,
        issues: Vec::new(),
    };
    let outline = code_index::Outline {
        path: "main.rs".into(),
        language: Some("rust".into()),
        file_revision: Some("a".repeat(64)),
        index_revision: 5,
        status: code_index::Status::Parsed,
        symbols: Vec::new(),
        capabilities: Default::default(),
        partial: false,
        truncated: false,
    };
    let lines = FileLines {
        path: "main.rs".into(),
        lines: Vec::new(),
        size: 0,
        revision: "a".repeat(64),
        offset: 2,
        next_offset: None,
    };
    let backend = Backend::new(vec![
        Output::CodeSearch(page.clone()),
        Output::CodeOutline(outline.clone()),
        Output::FileLines(lines.clone()),
    ]);
    let client = Client::new(backend.clone());
    assert_eq!(
        client
            .search_code(worktree, Default::default())
            .await
            .unwrap(),
        page
    );
    assert_eq!(
        client.outline_code(worktree, "main.rs").await.unwrap(),
        outline
    );
    assert_eq!(
        client
            .read_file_lines(worktree, "main.rs", 2, 20, Some("a".repeat(64)))
            .await
            .unwrap(),
        lines
    );
    let calls = backend.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0].command,
        Command::SearchCode {
            worktree,
            query: Default::default()
        }
    );
    assert_eq!(
        calls[2].command,
        Command::ReadFileLines {
            worktree,
            path: "main.rs".into(),
            offset: 2,
            limit: 20,
            expected_revision: Some("a".repeat(64)),
        }
    );
}

#[tokio::test]
async fn cancels_abandoned_reads_with_original_scope() {
    let backend = Backend::new(Vec::new());
    backend.block.store(true, Ordering::Relaxed);
    let client = Client::new(backend.clone());
    let worktree = WorktreeId::new();
    let scope = plugin::Context {
        invocation: None,
        turn: None,
        surface: plugin::desktop::Surface::Workspace,
        package: plugin::Reference {
            name: "code".into(),
            digest: "f".repeat(64),
            settings_revision: 2,
        },
        worktree: Some(worktree),
        session: None,
    };
    let request = client
        .prepare(Command::OutlineCode {
            worktree,
            path: "main.rs".into(),
        })
        .with_plugin(scope.clone());
    let id = request.id;
    let task = tokio::spawn(async move { client.execute(request).await });
    backend.entered.acquire().await.unwrap().forget();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(1), backend.cancelled.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let calls = backend.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].command,
        Command::CancelCodeIndexQuery { request: id }
    );
    assert_eq!(calls[1].plugin, Some(scope));
    assert_eq!(calls[1].target, backend.target());
}

#[tokio::test]
async fn rejects_mismatched_source_revisions() {
    let backend = Backend::new(vec![Output::FileLines(FileLines {
        path: "main.rs".into(),
        lines: Vec::new(),
        size: 0,
        revision: "b".repeat(64),
        offset: 0,
        next_offset: None,
    })]);
    let client = Client::new(backend);
    let error = client
        .read_file_lines(WorktreeId::new(), "main.rs", 0, 20, Some("a".repeat(64)))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}
