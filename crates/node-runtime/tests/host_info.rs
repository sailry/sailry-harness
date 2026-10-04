use std::time::{Duration, Instant};

use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, HostInfo, NodeId, Output, Request};

async fn inspect(client: &Client) -> HostInfo {
    let Output::HostInfo(info) = client
        .execute(client.prepare(Command::InspectHost))
        .await
        .unwrap()
    else {
        panic!("expected host information");
    };
    info
}

async fn metrics(client: &Client) -> sailry_protocol::host::metrics::Sample {
    let admission = client
        .dispatch(client.prepare(Command::ReadHostMetrics))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::HostMetrics(sample) = admission.completion.await.unwrap().unwrap() else {
        panic!("expected host metrics");
    };
    sample
}

#[tokio::test]
async fn shares_transient_inspection() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let info = controller.handle().inspect(&address).await.unwrap();
    assert!(info.execution && info.latency > Duration::ZERO);
    controller.handle().set_name("Test phone".into()).unwrap();
    let info = node
        .link()
        .inspect(&controller.handle().address())
        .await
        .unwrap();
    assert!(!info.execution && info.latency > Duration::ZERO);
    assert_eq!(info.name.as_deref(), Some("Test phone"));
    let local = Client::new(node.local());
    let transport = controller.handle().remote(address);
    let remote = Client::new(transport.clone());
    let first = inspect(&local).await;
    let second = inspect(&remote).await;
    assert_eq!(first.node, second.node);
    assert_eq!(first.name, second.name);
    assert_eq!(first.os, second.os);
    assert!(second.sampled_at_ms >= first.sampled_at_ms);
    assert_eq!(second.node, node.id());
    assert_ne!(
        second.node,
        NodeId(*controller.handle().address().id.as_bytes())
    );
    assert!(second.sampled_at_ms > 0);
    assert!(second.logical_cpus.is_some_and(|value| value > 0));
    let memory = second
        .memory
        .as_ref()
        .expect("test platform reports memory");
    assert!(memory.total_bytes > 0);
    assert!(memory.available_bytes <= memory.total_bytes);
    let initial = metrics(&local).await;
    assert!(initial.cpu_basis_points.is_none());
    assert!(initial.network.is_none());
    let routed = metrics(&remote).await;
    assert_eq!(routed.node, node.id());
    assert!(routed.sampled_at_ms >= initial.sampled_at_ms);
    assert!(initial.process_count >= initial.processes.len());
    assert!(!initial.processes.is_empty());
    assert!(!initial.disks.is_empty());

    let request = Request::new(node.id(), Command::InspectHost);
    let admission = transport.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    let database = rusqlite::Connection::open_with_flags(
        node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let rows: i64 = database
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    drop(database);

    tokio::time::sleep(Duration::from_millis(1100)).await;
    let fresh = inspect(&remote).await;
    assert!(fresh.sampled_at_ms > first.sampled_at_ms);
    assert!(fresh.node_uptime_secs >= 1);
    let mut sampled = metrics(&remote).await;
    // Startup discovery can leave a stale gap. Require a fresh utilization sample
    // after regular polling resumes, just as the host watcher does.
    let deadline = Instant::now() + Duration::from_secs(10);
    while sampled.cpu_basis_points.is_none() || sampled.network.is_none() {
        assert!(
            Instant::now() < deadline,
            "host utilization did not become available"
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        sampled = metrics(&remote).await;
    }
    assert!(sampled.sampled_at_ms > initial.sampled_at_ms);
    assert!(sampled.cpu_basis_points.is_some_and(|value| value <= 10000));
    assert!(sampled.network.is_some());
    let local_sample = metrics(&local).await;
    assert!(local_sample.network.is_some());
    if local_sample.sampled_at_ms == sampled.sampled_at_ms {
        assert_eq!(local_sample.network, sampled.network);
    }
    assert!(
        sampled
            .processes
            .iter()
            .any(|process| process.pid == std::process::id())
    );
    assert!(
        sampled
            .gpus
            .iter()
            .all(|gpu| gpu.usage_basis_points.is_none_or(|value| value <= 10000))
    );
    assert!(
        sampled
            .disks
            .iter()
            .all(|disk| disk.available_bytes <= disk.total_bytes)
    );
    println!(
        "Host sample: {} processes, {} disks with I/O, {} GPUs with utilization",
        sampled.process_count,
        sampled
            .disks
            .iter()
            .filter(|disk| disk.io.is_some())
            .count(),
        sampled
            .gpus
            .iter()
            .filter(|gpu| gpu.usage_basis_points.is_some())
            .count()
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    assert!(
        local
            .execute(local.prepare(Command::InspectHost))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejects_unauthorized_controllers() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let wrong = Request::new(NodeId([0; 32]), Command::InspectHost);
    assert_eq!(
        local.execute(wrong).await.unwrap_err().code,
        ErrorCode::WrongTarget
    );
    let mut wrong_version = local.prepare(Command::InspectHost);
    wrong_version.version = u16::MAX;
    assert_eq!(
        local.execute(wrong_version).await.unwrap_err().code,
        ErrorCode::WrongTarget
    );

    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    inspect(&remote).await;
    node.link()
        .set_trust(NodeId(*controller.handle().address().id.as_bytes()), false)
        .await
        .unwrap();
    assert!(
        remote
            .execute(remote.prepare(Command::InspectHost))
            .await
            .is_err()
    );
    assert!(
        remote
            .execute(remote.prepare(Command::ReadHostMetrics))
            .await
            .is_err()
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
