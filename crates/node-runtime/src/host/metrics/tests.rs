use super::*;

fn cached() -> Sample {
    Sample {
        sampled_at_ms: 1,
        cpu_basis_points: Some(1234),
        core_basis_points: vec![1234],
        memory: Some(MemoryUsage {
            total_bytes: 10,
            available_bytes: 6,
            free_bytes: 2,
        }),
        load: Some(LoadAverage {
            one: 100,
            five: 200,
            fifteen: 300,
        }),
        process_count: 1,
        processes: vec![Process {
            pid: 123,
            started_at_secs: 1,
            name: "Fixture".into(),
            cpu_basis_points: Some(1234),
            memory_bytes: 1024,
        }],
        disks: vec![Disk {
            mount: "/".into(),
            name: "Fixture".into(),
            file_system: "fixture".into(),
            total_bytes: 100,
            available_bytes: 50,
            io: None,
        }],
        network: Some(NetworkRate {
            received_bytes_per_sec: 1,
            transmitted_bytes_per_sec: 2,
            received_bytes: 10,
            transmitted_bytes: 20,
        }),
        ..empty(NodeId([1; 32]))
    }
}

#[test]
fn samples_only_basic_resources() {
    let mut sampler = Sampler::new();
    let sample = sampler.sample(NodeId([1; 32]), false).unwrap();
    assert!(sample.memory.is_some());
    assert!(sample.cpu_basis_points.is_none());
    assert!(sample.core_basis_points.is_empty());
    assert!(sample.load.is_none());
    assert!(sample.swap.is_none());
    assert!(sample.processes.is_empty());
    assert!(sample.disks.is_empty());
    assert!(sample.gpus.is_empty());
    assert!(sample.network.is_none());
    assert!(sampler.system.processes().is_empty());
    assert!(sampler.processes.processes().is_empty());
    assert!(sampler.disks.is_empty());
    assert!(sampler.networks.iter().next().is_none());
    assert!(sampler.sampled.is_none());
}

#[test]
fn projects_basic_cache() {
    let mut sampler = Sampler::new();
    let time = Instant::now();
    let cached = cached();
    sampler.last = Some((time, cached.clone()));
    sampler.sampled = Some(time);
    let sample = sampler.sample(cached.node, false).unwrap();
    assert_eq!(
        sample,
        Sample {
            sampled_at_ms: cached.sampled_at_ms,
            cpu_basis_points: cached.cpu_basis_points,
            memory: cached.memory.clone(),
            ..empty(cached.node)
        }
    );
    assert_eq!(sampler.last, Some((time, cached)));
    assert_eq!(sampler.sampled, Some(time));
    assert!(sampler.system.processes().is_empty());
    assert!(sampler.processes.processes().is_empty());
}

#[test]
fn upgrades_fresh_core() {
    let mut sampler = Sampler::new();
    let time = Instant::now();
    let cached = cached();
    sampler.last = Some((time, cached.clone()));
    let sample = sampler.sample(cached.node, true).unwrap();
    assert_eq!(sample.cpu_basis_points, cached.cpu_basis_points);
    assert_eq!(sample.memory, cached.memory);
    assert_eq!(sampler.last.as_ref().unwrap().0, time);
    assert!(sampler.sampled.is_some());
    assert!(!sample.processes.is_empty());
}

#[test]
fn preserves_detailed_cadence() {
    let mut sampler = Sampler::new();
    let time = Instant::now();
    let core = time - Duration::from_secs(2);
    let cached = cached();
    sampler.last = Some((core, cached.clone()));
    sampler.sampled = Some(time);
    let sample = sampler.sample(cached.node, true).unwrap();
    assert!(sample.memory.is_some());
    assert_eq!(sample.processes, cached.processes);
    assert_eq!(sample.disks, cached.disks);
    assert_eq!(sample.network, cached.network);
    assert_eq!(sampler.sampled, Some(time));
    assert!(sampler.last.as_ref().unwrap().0 > core);
    assert!(sampler.system.processes().is_empty());
    assert!(sampler.processes.processes().is_empty());
    assert!(sampler.disks.is_empty());
    assert!(sampler.networks.iter().next().is_none());
}
