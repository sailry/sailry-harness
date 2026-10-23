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
            if matches!(request.command, Command::ReportDiagnostic(_)) {
                let (sender, completion) = tokio::sync::oneshot::channel();
                sender.send(Ok(Output::NotificationsUpdated)).unwrap();
                return Ok(Admission {
                    receipt: Receipt {
                        id: request.id,
                        durable: true,
                    },
                    completion,
                });
            }
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
    let batch = next(&mut receiver).await;
    assert!(!batch.retrying);
    assert_eq!(batch.results, vec![("ssh".into(), Ok(package))]);
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["ssh", "ssh", "ssh"]);
}

#[tokio::test]
async fn isolates_failed_and_pending_packages() {
    let entered = CancellationToken::new();
    let stopped = CancellationToken::new();
    let mut unrelated = info("disabled");
    unrelated.summary.enabled = false;
    let healthy = info("worktrees");
    let reads = Reads::new(vec![
        ("disabled", vec![failure(ErrorCode::NotFound)]),
        (
            "held",
            vec![Step::Hold {
                entered: entered.clone(),
                release: CancellationToken::new(),
                dropped: stopped.clone(),
                response: Ok(Output::Plugin(info("held"))),
            }],
        ),
        ("worktrees", vec![success(&healthy)]),
    ]);
    let stop = CancellationToken::new();
    let (job, mut receiver) = start(
        reads,
        vec![
            unrelated.summary,
            info("held").summary,
            healthy.summary.clone(),
        ],
        stop.clone(),
    );
    let mut results = BTreeMap::new();
    for _ in 0..2 {
        let batch = next(&mut receiver).await;
        assert!(batch.retrying);
        results.extend(batch.results);
    }
    assert_eq!(results["worktrees"], Ok(healthy));
    assert_eq!(results["disabled"], Err("plugins_read_failed"));
    assert!(entered.is_cancelled());
    stop.cancel();
    finish(job).await;
    assert!(stopped.is_cancelled());
}

#[tokio::test]
async fn bounds_transient_attempts() {
    let reads = Reads::new(vec![(
        "ssh",
        vec![
            failure(ErrorCode::Busy),
            failure(ErrorCode::Unavailable),
            failure(ErrorCode::Busy),
        ],
    )]);
    let (job, mut receiver) = start(
        reads.clone(),
        vec![info("ssh").summary],
        CancellationToken::new(),
    );
    let batch = next(&mut receiver).await;
    assert!(!batch.retrying);
    assert_eq!(batch.diagnostics["ssh"].code, ErrorCode::Busy);
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["ssh", "ssh", "ssh"]);
}

#[tokio::test]
async fn rejects_changed_revisions_without_retry() {
    let expected = info("git");
    let mut changed = expected.clone();
    changed.summary.revision += 1;
    let reads = Reads::new(vec![("git", vec![success(&changed)])]);
    let (job, mut receiver) = start(
        reads.clone(),
        vec![expected.summary],
        CancellationToken::new(),
    );
    assert_eq!(
        next(&mut receiver).await.results,
        vec![("git".into(), Err("plugins_conflict"))]
    );
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["git"]);
}

#[tokio::test]
async fn keeps_unknown_outcomes_stable() {
    let reads = Reads::new(vec![("ssh", vec![failure(ErrorCode::OutcomeUnknown)])]);
    let (job, mut receiver) = start(
        reads.clone(),
        vec![info("ssh").summary],
        CancellationToken::new(),
    );
    assert!(!next(&mut receiver).await.retrying);
    finish(job).await;
    assert_eq!(*reads.requests.lock().unwrap(), ["ssh"]);
}

#[tokio::test]
async fn cancels_pending_reads_and_backoff() {
    for held in [false, true] {
        let dropped = CancellationToken::new();
        let step = if held {
            Step::Hold {
                entered: CancellationToken::new(),
                release: CancellationToken::new(),
                dropped: dropped.clone(),
                response: Ok(Output::Plugin(info("ssh"))),
            }
        } else {
            failure(ErrorCode::Unavailable)
        };
        let reads = Reads::new(vec![("ssh", vec![step])]);
        let stop = CancellationToken::new();
        let (job, mut receiver) = start(reads.clone(), vec![info("ssh").summary], stop.clone());
        reads.wait_count(1).await;
        stop.cancel();
        finish(job).await;
        assert!(receiver.recv().await.is_none());
        assert_eq!(*reads.requests.lock().unwrap(), ["ssh"]);
        if held {
            assert!(dropped.is_cancelled());
        }
    }
}

fn cache() -> Metadata {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
    );
    Metadata::new(Arc::new(Client::new(Reads::new(Vec::new()))), runtime)
}

fn connected(name: &str) -> Info {
    let mut value = info(name);
    value.extension = Some(serde_json::from_value(serde_json::json!({
        "api_version": "v1", "connectors": [{
            "id":"account", "display":{"label":"Account"}, "accounts":"single",
            "origins":["https://example.com"], "auth":[{"kind":"secret","fields":["token"]}],
            "fields":[{"name":"token","title":"Token","secret":true}],
            "inject":[{"server":"server","header":"Authorization","field":"token","prefix":"Bearer "}]
        }]
    })).unwrap());
    value.mcp = ["server", "anonymous"]
        .map(|name| sailry_protocol::plugin::Mcp {
            name: name.into(),
            host: None,
            transport: sailry_protocol::plugin::McpTransport::StreamableHttp,
            allowed: true,
            availability: sailry_protocol::plugin::mcp::Availability::Available,
        })
        .into();
    value
}

#[test]
fn invalidates_availability_at_unchanged_revisions() {
    use sailry_protocol::plugin::mcp::Availability;
    let mut cache = cache();
    let github = connected("github");
    let unrelated = connected("other");
    cache.expected = vec![github.summary.clone(), unrelated.summary.clone()];
    cache.entries = BTreeMap::from([
        ("github".into(), github.clone()),
        ("other".into(), unrelated.clone()),
    ]);
    let stale = cache.stop.child_token();
    let unaffected = cache.stop.child_token();
    for (summary, stop) in [
        (github.summary.clone(), stale.clone()),
        (unrelated.summary.clone(), unaffected.clone()),
    ] {
        cache.reads.insert(
            summary.name.clone(),
            Read {
                expected: summary,
                stop,
                pending: true,
                _task: None,
            },
        );
    }
    let expected = cache.reset(&BTreeSet::from(["github".into()]));
    assert_eq!(expected, [github.summary.clone()]);
    assert!(stale.is_cancelled());
    assert!(!unaffected.is_cancelled());
    assert!(!cache.reads.contains_key("github"));
    assert!(cache.reads["other"].pending);
    assert_eq!(cache.entries["other"], unrelated);
    let projected = &cache.entries["github"];
    assert_eq!(projected.summary, github.summary);
    assert_eq!(projected.extension, github.extension);
    assert_eq!(projected.mcp_source, github.mcp_source);
    assert_eq!(projected.mcp[0].availability, Availability::Unavailable);
    assert_eq!(projected.mcp[1].availability, Availability::Available);
}

#[test]
fn rejects_reads_from_previous_authorization() {
    use sailry_protocol::plugin::mcp::Availability;
    let mut cache = cache();
    let previous = connected("github");
    cache.expected = vec![previous.summary.clone()];
    cache.entries.insert("github".into(), previous.clone());
    let stale = cache.stop.child_token();
    cache.reads.insert(
        "github".into(),
        Read {
            expected: previous.summary.clone(),
            stop: stale.clone(),
            pending: true,
            _task: None,
        },
    );
    assert_eq!(
        cache.reset(&BTreeSet::from(["github".into()])),
        [previous.summary.clone()]
    );
    let mut current = previous.clone();
    current.mcp[0].availability = Availability::ConnectionRequired;
    let fresh = cache.stop.child_token();
    assert!(cache.apply(
        Batch {
            results: vec![("github".into(), Ok(current.clone()))],
            diagnostics: Default::default(),
            retrying: false,
        },
        &fresh
    ));
    assert!(!cache.apply(
        Batch {
            results: vec![("github".into(), Ok(previous))],
            diagnostics: Default::default(),
            retrying: false,
        },
        &stale
    ));
    assert_eq!(cache.entries["github"], current);
}

#[test]
fn refreshes_pending_reads_and_retains_api_declarations() {
    let mut cache = cache();
    let pending = info("github");
    let api = info("figma");
    cache.expected = vec![pending.summary.clone(), api.summary.clone()];
    cache.entries.insert("figma".into(), api.clone());
    cache.errors.insert("github".into(), "plugins_read_failed");
    let stale = cache.stop.child_token();
    cache.reads.insert(
        "github".into(),
        Read {
            expected: pending.summary.clone(),
            stop: stale.clone(),
            pending: true,
            _task: None,
        },
    );
    assert_eq!(
        cache.reset(&BTreeSet::from(["github".into(), "figma".into()])),
        [pending.summary]
    );
    assert!(stale.is_cancelled());
    assert!(!cache.errors.contains_key("github"));
    assert_eq!(cache.entries["figma"], api);
}

async fn read(cache: &mut Metadata, expected: Summary) -> CancellationToken {
    let stop = cache.stop.child_token();
    cache.reads.insert(
        expected.name.clone(),
        Read {
            expected: expected.clone(),
            stop: stop.clone(),
            pending: true,
            _task: None,
        },
    );
    let (sender, mut receiver) = mpsc::channel(1);
    let job = tokio::spawn(load(
        cache.client.clone(),
        vec![expected],
        stop.clone(),
        sender,
    ));
    assert!(cache.apply(next(&mut receiver).await, &stop));
    finish(job).await;
    stop
}

#[test]
fn retries_failed_refreshes_with_unchanged_inventory() {
    use sailry_protocol::plugin::mcp::Availability;
    for loaded in [true, false] {
        let mut previous = connected("github");
        previous.mcp[0].availability = Availability::ConnectionRequired;
        let mut current = previous.clone();
        current.mcp[0].availability = Availability::Available;
        let unrelated = connected("other");
        let mut steps = Vec::new();
        if loaded {
            steps.push(success(&previous));
        }
        steps.extend([
            failure(ErrorCode::Unavailable),
            failure(ErrorCode::Unavailable),
            failure(ErrorCode::Unavailable),
            success(&current),
        ]);
        let reads = Reads::new(vec![("github", steps)]);
        let mut cache = cache();
        cache.client = Arc::new(Client::new(reads.clone()));
        cache.expected = vec![previous.summary.clone(), unrelated.summary.clone()];
        cache.entries.insert("other".into(), unrelated.clone());
        let runtime = cache.runtime.clone();
        runtime.block_on(async {
            if loaded {
                read(&mut cache, previous.summary.clone()).await;
                assert_eq!(cache.entries["github"], previous);
            }
            assert_eq!(
                cache.reset(&BTreeSet::from(["github".into()])),
                [previous.summary.clone()]
            );
            let failed = read(&mut cache, previous.summary.clone()).await;
            assert_eq!(cache.errors["github"], "plugins_read_failed");
            assert_eq!(cache.diagnostics["github"].code, ErrorCode::Unavailable);
            assert!(!cache.reads["github"].pending);
            if loaded {
                let retained = &cache.entries["github"];
                assert_eq!(retained.summary, previous.summary);
                assert_eq!(retained.extension, previous.extension);
                assert_eq!(retained.mcp[0].availability, Availability::Unavailable);
            } else {
                assert!(!cache.entries.contains_key("github"));
            }

            let expected = cache.prepare_retry(None);
            assert_eq!(expected, [previous.summary.clone()]);
            assert!(failed.is_cancelled());
            assert!(!cache.errors.contains_key("github"));
            assert!(!cache.diagnostics.contains_key("github"));
            assert_eq!(cache.entries["other"], unrelated);
            for expected in expected {
                read(&mut cache, expected).await;
            }
            assert_eq!(cache.entries["github"], current);
            assert!(cache.errors.is_empty());
            assert!(cache.diagnostics.is_empty());
            assert!(cache.settled());
            assert!(!cache.apply(
                Batch {
                    results: vec![("github".into(), Ok(previous.clone()))],
                    diagnostics: Default::default(),
                    retrying: false,
                },
                &failed
            ));
            assert_eq!(cache.entries["github"], current);
        });
        assert_eq!(
            reads.requests.lock().unwrap().len(),
            if loaded { 5 } else { 4 }
        );
        assert!(
            reads
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|name| name == "github")
        );
    }
}

#[test]
fn retries_selected_failures_without_disrupting_pending_reads() {
    let mut cache = cache();
    let first = connected("first");
    let second = connected("second");
    let pending = connected("pending");
    let ready = connected("ready");
    cache.expected = [&first, &second, &pending, &ready]
        .map(|info| info.summary.clone())
        .into();
    cache.entries = [&first, &second, &ready]
        .into_iter()
        .map(|info| (info.summary.name.clone(), info.clone()))
        .collect();
    let mut stops = BTreeMap::new();
    for info in [&first, &second, &pending] {
        let stop = cache.stop.child_token();
        stops.insert(info.summary.name.clone(), stop.clone());
        cache.reads.insert(
            info.summary.name.clone(),
            Read {
                expected: info.summary.clone(),
                stop,
                pending: info.summary == pending.summary,
                _task: None,
            },
        );
    }
    for info in [&first, &second] {
        cache
            .errors
            .insert(info.summary.name.clone(), "plugins_read_failed");
        cache.diagnostics.insert(
            info.summary.name.clone(),
            Fault::new(ErrorCode::Unavailable, "injected metadata failure"),
        );
    }
    assert_eq!(cache.prepare_retry(Some("first")), [first.summary.clone()]);
    assert!(stops["first"].is_cancelled());
    assert!(!stops["second"].is_cancelled());
    assert!(!stops["pending"].is_cancelled());
    assert!(cache.errors.contains_key("second"));
    assert!(cache.diagnostics.contains_key("second"));
    assert!(cache.reads["pending"].pending);
    assert_eq!(cache.entries["ready"], ready);
    let retry = cache.stop.child_token();
    cache.reads.insert(
        "first".into(),
        Read {
            expected: first.summary.clone(),
            stop: retry.clone(),
            pending: true,
            _task: None,
        },
    );
    assert!(cache.prepare_retry(Some("first")).is_empty());
    assert!(!retry.is_cancelled());
    assert!(cache.reads["first"].pending);
    assert_eq!(cache.prepare_retry(None), [second.summary.clone()]);
    assert!(stops["second"].is_cancelled());
    assert!(!stops["pending"].is_cancelled());
    assert_eq!(cache.entries["first"], first);
    assert_eq!(cache.entries["ready"], ready);
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
    assert!(cache.settled());
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

#[test]
fn readiness_is_scoped_to_exact_inventory() {
    let mut cache = cache();
    let healthy = info("worktrees");
    let unrelated = info("disabled");
    cache.expected = vec![healthy.summary.clone(), unrelated.summary];
    cache.entries.insert("worktrees".into(), healthy.clone());
    cache
        .errors
        .insert("disabled".into(), "plugins_read_failed");
    cache.reads.insert(
        "disabled".into(),
        Read {
            expected: info("disabled").summary,
            stop: cache.stop.child_token(),
            pending: true,
            _task: None,
        },
    );
    assert!(cache.ready(&healthy.summary.reference()));
    cache.expected[0].revision += 1;
    assert!(!cache.ready(&healthy.summary.reference()));
    cache.expected[0] = healthy.summary.clone();
    let mut stale = healthy.summary.reference();
    stale.digest.push('x');
    assert!(!cache.ready(&stale));
    stale = healthy.summary.reference();
    stale.settings_revision += 1;
    assert!(!cache.ready(&stale));
}

#[test]
fn preserves_pending_reads_across_unrelated_changes() {
    let mut cache = cache();
    let held = info("held").summary;
    let mut other = info("other").summary;
    cache.expected = vec![held.clone(), other.clone()];
    let stop = cache.stop.child_token();
    cache.reads.insert(
        held.name.clone(),
        Read {
            expected: held.clone(),
            stop: stop.clone(),
            pending: true,
            _task: None,
        },
    );
    other.revision += 1;
    assert!(cache.reconcile(&[held.clone(), other.clone()]));
    assert!(!stop.is_cancelled());
    assert!(cache.reads["held"].pending);
    assert!(!cache.reconcile(&[held.clone(), other.clone()]));
    let mut replacement = held;
    replacement.revision += 1;
    assert!(cache.reconcile(&[replacement, other]));
    assert!(stop.is_cancelled());
    assert!(!cache.reads.contains_key("held"));
}
