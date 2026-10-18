use super::*;
use core::prelude::v1::test;
use loading::{Batch, load};
use sailry_link::{Admission, Pending, Response, Subscription, Transport};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Receipt, Request, Topic};
use std::{collections::VecDeque, sync::Mutex, time::Duration};
use tokio::sync::{Notify, mpsc};

enum Step {
    Reply(Response),
    Hold {
        entered: CancellationToken,
        release: CancellationToken,
        dropped: CancellationToken,
        response: Response,
    },
}

struct Waiting(CancellationToken);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct Reads {
    steps: Mutex<BTreeMap<String, VecDeque<Step>>>,
    requests: Mutex<Vec<String>>,
    changed: Notify,
}

impl Reads {
    fn new(steps: Vec<(&str, Vec<Step>)>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(
                steps
                    .into_iter()
                    .map(|(name, steps)| (name.into(), steps.into()))
                    .collect(),
            ),
            requests: Mutex::new(Vec::new()),
            changed: Notify::new(),
        })
    }

    async fn wait_count(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let changed = self.changed.notified();
                if self.requests.lock().unwrap().len() == expected {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("metadata request deadline");
    }
}

impl Transport for Reads {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            assert!(!request.command.durable());
            let Command::ReadPlugin { name } = request.command else {
                panic!("plugin metadata read expected");
            };
            assert_eq!(request.target, self.target());
            self.requests.lock().unwrap().push(name.clone());
            self.changed.notify_one();
            let step = self
                .steps
                .lock()
                .unwrap()
                .get_mut(&name)
                .and_then(VecDeque::pop_front)
                .expect("unexpected metadata read");
            let response = match step {
                Step::Reply(response) => response,
                Step::Hold {
                    entered,
                    release,
                    dropped,
                    response,
                } => {
                    let _waiting = Waiting(dropped);
                    entered.cancel();
                    release.cancelled().await;
                    response
                }
            };
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(response).unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }

    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        unreachable!("metadata reads do not subscribe")
    }
}

fn info(name: &str) -> Info {
    Info {
        summary: Summary {
            name: name.into(),
            revision: 1,
            digest: format!("{name}-digest"),
            settings_revision: 0,
            enabled: true,
            version: Some("0.1.0".into()),
            description: None,
        },
        origin: None,
        release_notes: None,
        icon: None,
        skill: None,
        mcp_source: None,
        extension: None,
        skills: Vec::new(),
        mcp: Vec::new(),
        settings: None,
        issues: Vec::new(),
    }
}

fn success(info: &Info) -> Step {
    Step::Reply(Ok(Output::Plugin(info.clone())))
}

fn failure(code: ErrorCode) -> Step {
    Step::Reply(Err(Fault::new(code, "injected metadata failure")))
}

fn start(
    reads: Arc<Reads>,
    expected: Vec<Summary>,
    stop: CancellationToken,
) -> (tokio::task::JoinHandle<()>, mpsc::Receiver<Batch>) {
    let client = Arc::new(Client::new(reads));
    let (sender, receiver) = mpsc::channel(1);
    (tokio::spawn(load(client, expected, stop, sender)), receiver)
}

async fn next(receiver: &mut mpsc::Receiver<Batch>) -> Batch {
    tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("metadata result deadline")
        .expect("metadata batch expected")
}

async fn finish(job: tokio::task::JoinHandle<()>) {
    tokio::time::timeout(Duration::from_secs(2), job)
        .await
        .expect("metadata cancellation deadline")
        .unwrap();
}

#[tokio::test]
async fn recovers_with_unchanged_inventory() {
    let package = info("ssh");
    let reads = Reads::new(vec![(
        "ssh",
        vec![
            failure(ErrorCode::Busy),
            failure(ErrorCode::Unavailable),
            success(&package),
        ],
    )]);
    let (job, mut receiver) = start(
        reads.clone(),
        vec![package.summary.clone()],
        CancellationToken::new(),
    );

    // One inventory load recovers without another accept or manual refresh.
    for code in [ErrorCode::Busy, ErrorCode::Unavailable] {
        let batch = next(&mut receiver).await;
        assert!(batch.retrying);
        assert_eq!(
            batch.diagnostics.get("ssh"),
            Some(&Fault::new(code, "injected metadata failure"))
        );
        assert_eq!(
            batch.results,
            vec![("ssh".into(), Err("plugins_read_failed"))]
        );
    }
    let batch = next(&mut receiver).await;
    assert!(!batch.retrying);
    assert!(batch.diagnostics.is_empty());
    assert_eq!(batch.results, vec![("ssh".into(), Ok(package))]);
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["ssh", "ssh", "ssh"]);
}

#[tokio::test]
async fn retries_only_transient_entries() {
    let files = info("files");
    let ssh = info("ssh");
    let databases = info("databases");
    let git = info("git");
    let mut stale = git.clone();
    stale.summary.revision += 1;
    let reads = Reads::new(vec![
        ("files", vec![success(&files)]),
        ("ssh", vec![failure(ErrorCode::Busy), success(&ssh)]),
        ("databases", vec![failure(ErrorCode::NotFound)]),
        ("git", vec![success(&stale)]),
    ]);
    let expected = [&files, &ssh, &databases, &git]
        .map(|info| info.summary.clone())
        .to_vec();
    let (job, mut receiver) = start(reads.clone(), expected, CancellationToken::new());

    let batch = next(&mut receiver).await;
    assert!(batch.retrying);
    assert_eq!(
        batch.results,
        vec![
            ("files".into(), Ok(files)),
            ("ssh".into(), Err("plugins_read_failed")),
            ("databases".into(), Err("plugins_read_failed")),
            ("git".into(), Err("plugins_conflict")),
        ]
    );
    let batch = next(&mut receiver).await;
    assert!(!batch.retrying);
    assert_eq!(batch.results, vec![("ssh".into(), Ok(ssh))]);
    finish(job).await;
    assert_eq!(
        *reads.requests.lock().unwrap(),
        ["files", "ssh", "databases", "git", "ssh"]
    );
}

#[tokio::test]
async fn keeps_unknown_outcomes_stable() {
    let reads = Reads::new(vec![("ssh", vec![failure(ErrorCode::OutcomeUnknown)])]);
    let (job, mut receiver) = start(
        reads.clone(),
        vec![info("ssh").summary],
        CancellationToken::new(),
    );
    let batch = next(&mut receiver).await;
    assert!(!batch.retrying);
    assert_eq!(
        batch.results,
        vec![("ssh".into(), Err("plugins_read_failed"))]
    );
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["ssh"]);
}

#[tokio::test]
async fn cancels_pending_reads() {
    let entered = CancellationToken::new();
    let release = CancellationToken::new();
    let dropped = CancellationToken::new();
    let reads = Reads::new(vec![(
        "ssh",
        vec![Step::Hold {
            entered: entered.clone(),
            release,
            dropped: dropped.clone(),
            response: Ok(Output::Plugin(info("ssh"))),
        }],
    )]);
    let stop = CancellationToken::new();
    let (job, mut receiver) = start(reads, vec![info("ssh").summary], stop.clone());
    tokio::time::timeout(Duration::from_secs(2), entered.cancelled())
        .await
        .unwrap();
    stop.cancel();
    finish(job).await;
    assert!(dropped.is_cancelled());
    assert!(receiver.recv().await.is_none());
}

#[tokio::test]
async fn cancels_retry_backoff() {
    let reads = Reads::new(vec![("ssh", vec![failure(ErrorCode::Unavailable)])]);
    let stop = CancellationToken::new();
    let (job, mut receiver) = start(reads.clone(), vec![info("ssh").summary], stop.clone());
    assert!(next(&mut receiver).await.retrying);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), receiver.recv())
            .await
            .is_err()
    );
    stop.cancel();
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["ssh"]);
    assert!(receiver.recv().await.is_none());
}

#[tokio::test]
async fn cancels_blocked_result_delivery() {
    let reads = Reads::new(vec![(
        "ssh",
        vec![failure(ErrorCode::Busy), success(&info("ssh"))],
    )]);
    let stop = CancellationToken::new();
    let (job, mut receiver) = start(reads.clone(), vec![info("ssh").summary], stop.clone());
    reads.wait_count(2).await;
    // The first failed batch still occupies the bounded result channel.
    stop.cancel();
    finish(job).await;
    assert!(receiver.recv().await.unwrap().retrying);
    assert!(receiver.recv().await.is_none());
}

fn cache() -> Metadata {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap(),
    );
    Metadata::new(Arc::new(Client::new(Reads::new(Vec::new()))), runtime)
}

#[test]
fn clears_only_recovered_errors() {
    let mut cache = cache();
    let files = info("files");
    cache.entries.insert("files".into(), files.clone());
    cache.errors = BTreeMap::from([
        ("ssh".into(), "plugins_read_failed"),
        ("databases".into(), "plugins_read_failed"),
    ]);
    let fault = Fault::new(ErrorCode::Busy, "metadata storage queue is full");
    cache.diagnostics = BTreeMap::from([
        ("ssh".into(), fault.clone()),
        ("databases".into(), fault.clone()),
    ]);
    let ssh = info("ssh");
    assert!(cache.apply(
        Batch {
            results: vec![("ssh".into(), Ok(ssh.clone()))],
            diagnostics: Default::default(),
            retrying: false,
        },
        &cache.stop.clone(),
    ));
    assert_eq!(cache.entries["files"], files);
    assert_eq!(cache.entries["ssh"], ssh);
    assert_eq!(
        cache.errors,
        BTreeMap::from([("databases".into(), "plugins_read_failed")])
    );
    assert!(!cache.settled());
    assert_eq!(
        cache.diagnostics,
        BTreeMap::from([("databases".into(), fault)])
    );
}

#[test]
fn ignores_cancelled_batches() {
    let mut cache = cache();
    let old = info("ssh");
    let mut current = old.clone();
    current.summary.revision += 1;
    cache.expected = vec![current.summary.clone()];
    cache.entries.insert("ssh".into(), current.clone());
    let stop = cache.stop.clone();
    stop.cancel();
    cache.stop = CancellationToken::new();
    assert!(!cache.apply(
        Batch {
            results: vec![("ssh".into(), Ok(old))],
            diagnostics: Default::default(),
            retrying: false,
        },
        &stop,
    ));
    assert_eq!(cache.entries["ssh"], current);
    assert!(cache.errors.is_empty());
}
