use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use tokio::sync::oneshot;

use super::Supervisor;
use crate::Error;

#[tokio::test]
async fn preserves_detached_work() {
    let supervisor = Supervisor::start(2);
    let completed = Arc::new(AtomicBool::new(false));
    let effect = completed.clone();
    let (release, released) = oneshot::channel();
    let observer = supervisor
        .tasks
        .spawn(async move {
            released.await.unwrap();
            effect.store(true, Ordering::SeqCst);
        })
        .unwrap();
    drop(observer);
    release.send(()).unwrap();
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
    assert!(completed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn bounds_admitted_work() {
    let supervisor = Supervisor::start(1);
    let (release, released) = oneshot::channel();
    let observer = supervisor
        .tasks
        .spawn(async move {
            released.await.unwrap();
        })
        .unwrap();
    assert!(matches!(supervisor.tasks.spawn(async {}), Err(Error::Busy)));
    release.send(()).unwrap();
    observer.await.unwrap();
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn supervises_children_beyond_generic_capacity() {
    let supervisor = Supervisor::start(1);
    let stop = sailry_link::CancellationToken::new();
    let release = sailry_link::CancellationToken::new();
    let held = release.clone();
    let ordinary = supervisor
        .tasks
        .spawn(async move { held.cancelled().await })
        .unwrap();
    let mut children = Vec::new();
    // The generic pool has 64 slots in a Node; it is not a child policy limit.
    for index in 0..65 {
        let held = release.clone();
        children.push(
            supervisor
                .tasks
                .spawn_child(&stop, async move {
                    held.cancelled().await;
                    index
                })
                .await
                .unwrap(),
        );
    }
    assert!(matches!(supervisor.tasks.spawn(async {}), Err(Error::Busy)));
    release.cancel();
    ordinary.await.unwrap();
    for (index, child) in children.into_iter().enumerate() {
        assert_eq!(child.await.unwrap(), index);
    }
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn rejects_stopped_children_before_execution() {
    let supervisor = Supervisor::start(1);
    let stop = sailry_link::CancellationToken::new();
    stop.cancel();
    let executed = Arc::new(AtomicBool::new(false));
    let observed = executed.clone();
    assert!(matches!(
        supervisor
            .tasks
            .spawn_child(&stop, async move {
                observed.store(true, Ordering::SeqCst);
            })
            .await,
        Err(Error::Stopped)
    ));
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
    assert!(!executed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn drains_on_shutdown() {
    let supervisor = Supervisor::start(2);
    let tasks = supervisor.tasks.clone();
    let (release, released) = oneshot::channel();
    let observer = tasks
        .spawn(async move {
            released.await.unwrap();
            42
        })
        .unwrap();
    let shutdown = tokio::spawn(supervisor.shutdown(Duration::from_secs(1)));
    tokio::time::timeout(Duration::from_secs(1), async {
        while !tasks.capacity.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(tasks.spawn(async {}), Err(Error::Stopped)));
    assert!(!shutdown.is_finished());
    release.send(()).unwrap();
    assert_eq!(observer.await.unwrap(), 42);
    shutdown.await.unwrap().unwrap();
}

#[tokio::test]
async fn joins_expired_work() {
    let supervisor = Supervisor::start(1);
    let observer = supervisor
        .tasks
        .spawn(std::future::pending::<()>())
        .unwrap();
    assert!(matches!(
        supervisor.shutdown(Duration::from_millis(20)).await,
        Err(Error::ShutdownTimeout)
    ));
    assert!(observer.await.is_err());
}

#[tokio::test]
async fn cleans_up_after_panic() {
    let supervisor = Supervisor::start(2);
    let observer = supervisor
        .tasks
        .spawn(async { panic!("injected worker failure") })
        .unwrap();
    assert!(observer.await.is_err());
    assert!(matches!(
        supervisor.shutdown(Duration::from_secs(1)).await,
        Err(Error::Worker(_))
    ));
}

#[tokio::test]
async fn cancels_on_drop() {
    let supervisor = Supervisor::start(1);
    let tasks = supervisor.tasks.clone();
    let observer = tasks.spawn(std::future::pending::<()>()).unwrap();
    drop(supervisor);
    assert!(matches!(tasks.spawn(async {}), Err(Error::Stopped)));
    assert!(
        tokio::time::timeout(Duration::from_secs(1), observer)
            .await
            .unwrap()
            .is_err()
    );
}
