use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{NodeId, Receipt, Request, Topic};
use std::sync::Mutex;

fn sample(time: u64, started: u64) -> Sample {
    Sample {
        node: NodeId([1; 32]),
        sampled_at_ms: time,
        cpu_basis_points: Some(3000),
        core_basis_points: vec![2000, 4000],
        load: None,
        memory: None,
        swap: None,
        gpus: Vec::new(),
        disks: Vec::new(),
        network: None,
        process_count: 1,
        processes: vec![Process {
            pid: 1,
            started_at_secs: started,
            name: "Fixture".into(),
            cpu_basis_points: Some(5000),
            memory_bytes: 1024,
        }],
    }
}

#[test]
fn isolates_reused_pids() {
    let mut view = View::default();
    for time in 1..=70 {
        view.accept(sample(time * 1000, 1)).unwrap();
    }
    assert_eq!(view.history.len(), HISTORY_LIMIT);
    assert_eq!(view.processes[&(1, 1)].len(), HISTORY_LIMIT);
    view.accept(sample(71_000, 2)).unwrap();
    assert_eq!(view.processes.len(), 1);
    assert_eq!(view.processes[&(1, 2)].len(), 1);
    let mut empty = sample(72_000, 2);
    empty.processes.clear();
    view.accept(empty).unwrap();
    assert!(view.processes.is_empty());
}

#[test]
fn ignores_duplicates_and_breaks_gaps() {
    let mut view = View::default();
    view.accept(sample(1000, 1)).unwrap();
    view.accept(sample(1000, 1)).unwrap();
    view.accept(sample(500, 1)).unwrap();
    assert_eq!(view.history.len(), 1);
    let mut wrong = sample(2000, 1);
    wrong.node = NodeId([2; 32]);
    assert_eq!(view.accept(wrong).unwrap_err().code, ErrorCode::WrongTarget);
    view.accept(sample(10_000, 1)).unwrap();
    assert_eq!(view.history.len(), 1);
    assert_eq!(view.processes[&(1, 1)].len(), 1);
}

#[test]
fn preserves_missing_samples() {
    let mut view = View::default();
    let mut first = sample(1000, 1);
    first.network = Some(NetworkRate {
        received_bytes_per_sec: 12345,
        transmitted_bytes_per_sec: 6789,
        received_bytes: 1 << 30,
        transmitted_bytes: 1 << 20,
    });
    let expected = first.network.clone();
    view.accept(first).unwrap();
    view.accept(sample(2000, 1)).unwrap();
    assert_eq!(view.history[0].network, expected);
    assert_eq!(view.history[1].network, None);
    assert_eq!(view.sample.as_ref().unwrap().network, None);
}

struct Feed {
    calls: Mutex<Vec<bool>>,
}

impl Transport for Feed {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let Command::ReadHostMetrics { detailed } = request.command else {
                panic!("host observation must request host metrics");
            };
            self.calls.lock().unwrap().push(detailed);
            let (ready, completion) = tokio::sync::oneshot::channel();
            ready
                .send(Ok(Output::HostMetrics(sample(1000, 1))))
                .unwrap();
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
        panic!("host observation must not subscribe to events");
    }
}

#[tokio::test(start_paused = true)]
async fn watches_requested_detail() {
    for detailed in [false, true] {
        let feed = Arc::new(Feed {
            calls: Mutex::new(Vec::new()),
        });
        let client = Client::new(feed.clone());
        let (updates, mut receiver) = watch::channel(View::default());
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let observer = tokio::spawn(async move {
            client.watch_host(detailed, updates, cancellation).await;
        });
        receiver.changed().await.unwrap();
        assert!(receiver.borrow().sample.is_some());
        if detailed {
            stop.cancel();
        } else {
            drop(receiver);
        }
        observer.await.unwrap();
        assert_eq!(*feed.calls.lock().unwrap(), [detailed]);
    }
}
