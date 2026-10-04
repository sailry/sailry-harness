use super::*;
use sailry_protocol::NodeId;

fn sample(time: u64, started: u64) -> Sample {
    Sample {
        node: NodeId([1; 32]),
        sampled_at_ms: time,
        cpu_basis_points: Some(3000),
        memory: None,
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
    });
    let expected = first.network.clone();
    view.accept(first).unwrap();
    view.accept(sample(2000, 1)).unwrap();
    assert_eq!(view.history[0].network, expected);
    assert_eq!(view.history[1].network, None);
    assert_eq!(view.sample.as_ref().unwrap().network, None);
}
