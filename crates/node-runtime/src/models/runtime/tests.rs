use super::*;
use crate::models::{registry::Registry, test_support::Fixture};
use std::sync::atomic::{AtomicUsize, Ordering};

async fn ready(owner: &Arc<Registry>, id: &str) -> Resource {
    owner.models(None).await.unwrap();
    owner.resource(Kind::Embedding, Some(id), None).unwrap()
}

#[tokio::test]
async fn serializes_work_and_prioritizes_interactive_queue() {
    let fixture = Fixture::new().await;
    let resource = ready(&fixture.owner, &fixture.record.id).await;
    let runtime = Arc::new(Runtime::new(CancellationToken::new()));
    let order = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (entered, entering) = oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let running = runtime.clone();
    let captured = resource.clone();
    let first_order = order.clone();
    let first_active = active.clone();
    let first_peak = peak.clone();
    let first = tokio::spawn(async move {
        running
            .execute(
                captured,
                Lane::Background,
                &CancellationToken::new(),
                Arc::new(|| Ok(())),
                move |_, _| {
                    first_peak.fetch_max(
                        first_active.fetch_add(1, Ordering::SeqCst) + 1,
                        Ordering::SeqCst,
                    );
                    let _ = entered.send(());
                    waiting.recv().unwrap();
                    first_order.lock().unwrap().push("first");
                    first_active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
    });
    entering.await.unwrap();
    let mut jobs = Vec::new();
    for (lane, label) in [
        (Lane::Background, "background"),
        (Lane::Interactive, "interactive"),
    ] {
        let running = runtime.clone();
        let captured = resource.clone();
        let order = order.clone();
        let active = active.clone();
        let peak = peak.clone();
        jobs.push(tokio::spawn(async move {
            running
                .execute(
                    captured,
                    lane,
                    &CancellationToken::new(),
                    Arc::new(|| Ok(())),
                    move |_, _| {
                        peak.fetch_max(active.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                        order.lock().unwrap().push(label);
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
        }));
    }
    tokio::task::yield_now().await;
    assert_eq!(runtime.interactive.capacity(), 7);
    assert_eq!(runtime.background.capacity(), 3);
    release.send(()).unwrap();
    first.await.unwrap().unwrap();
    for job in jobs {
        job.await.unwrap().unwrap();
    }
    assert_eq!(
        *order.lock().unwrap(),
        ["first", "interactive", "background"]
    );
    assert_eq!(peak.load(Ordering::SeqCst), 1);
    runtime.shutdown().await;
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn skips_cancelled_and_dropped_queued_work() {
    let fixture = Fixture::new().await;
    let resource = ready(&fixture.owner, &fixture.record.id).await;
    let runtime = Runtime::new(CancellationToken::new());
    let (entered, entering) = oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let (_running, finished) = runtime
        .submit(
            resource.clone(),
            Lane::Background,
            &CancellationToken::new(),
            Arc::new(|| Ok(())),
            false,
            move |_, _| {
                let _ = entered.send(());
                waiting.recv().unwrap();
                Ok(())
            },
        )
        .unwrap();
    entering.await.unwrap();
    let invoked = Arc::new(AtomicUsize::new(0));
    let stop = CancellationToken::new();
    let (pending, result) = runtime
        .submit(
            resource.clone(),
            Lane::Interactive,
            &stop,
            Arc::new(|| Ok(())),
            false,
            {
                let invoked = invoked.clone();
                move |_, _| {
                    invoked.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .unwrap();
    // A dropped caller owns a child cancellation token, independent of its parent.
    drop(pending);
    release.send(()).unwrap();
    finished.await.unwrap().unwrap();
    let result = result.await.unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(!stop.is_cancelled());
    stop.cancel();
    assert!(
        matches!(runtime.submit(resource,Lane::Background,&stop,Arc::new(||Ok(())),false,|_,_|Ok(())),Err(error) if error.code==ErrorCode::Cancelled)
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 0);
    runtime.shutdown().await;
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn cancels_active_work_with_its_owner() {
    let fixture = Fixture::new().await;
    let resource = ready(&fixture.owner, &fixture.record.id).await;
    for shutdown in [false, true] {
        let stop = CancellationToken::new();
        let runtime = Arc::new(Runtime::new(stop.clone()));
        let cancel = CancellationToken::new();
        let (entered, entering) = oneshot::channel();
        let (observed, observing) = oneshot::channel();
        let work = tokio::spawn({
            let runtime = runtime.clone();
            let resource = resource.clone();
            let cancel = cancel.clone();
            async move {
                runtime
                    .execute(
                        resource,
                        Lane::Interactive,
                        &cancel,
                        Arc::new(|| Ok(())),
                        move |_, cancel| {
                            let _ = entered.send(());
                            while !cancel.is_cancelled() {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            let _ = observed.send(());
                            Err::<(), _>(cancelled())
                        },
                    )
                    .await
            }
        });
        entering.await.unwrap();
        if shutdown {
            stop.cancel();
        } else {
            cancel.cancel();
        }
        assert_eq!(work.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
        tokio::time::timeout(Duration::from_secs(2), observing)
            .await
            .unwrap()
            .unwrap();
        runtime.shutdown().await;
    }
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn evicts_idle_and_least_recently_used_engines() {
    let fixture = Fixture::new().await;
    let resource = ready(&fixture.owner, &fixture.record.id).await;
    let now = Instant::now();
    let entry = |used, bytes| Loaded {
        resource: resource.clone(),
        used,
        bytes,
        engine: Engine::load(&resource).unwrap(),
    };
    let mut loaded = BTreeMap::from([
        ("old".into(), entry(now - IDLE, 500)),
        ("recent".into(), entry(now, 500)),
    ]);
    expire(&mut loaded, now);
    assert_eq!(loaded.keys().cloned().collect::<Vec<_>>(), ["recent"]);
    loaded.insert(
        "older".into(),
        entry(now - Duration::from_secs(1), MEMORY_BYTES / 2),
    );
    make_room(&mut loaded, MEMORY_BYTES / 2);
    assert_eq!(loaded.keys().cloned().collect::<Vec<_>>(), ["recent"]);
    make_room(&mut loaded, MEMORY_BYTES + 1);
    assert!(loaded.is_empty());
    // An oversized model is admitted alone; the next acquisition can still evict it.
    loaded.insert("oversized".into(), entry(now, MEMORY_BYTES + 1));
    make_room(&mut loaded, 1);
    assert!(loaded.is_empty());
    fixture.owner.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn wakes_at_the_idle_deadline() {
    let (_interactive, mut interactive) = mpsc::channel(8);
    let (_background, mut background) = mpsc::channel(4);
    let (_control, mut control) = mpsc::channel(4);
    let stop = CancellationToken::new();
    let deadline = Instant::now() + IDLE;
    let waiting = next(
        &mut interactive,
        &mut background,
        &mut control,
        &stop,
        deadline,
    );
    tokio::pin!(waiting);
    assert!(futures::poll!(&mut waiting).is_pending());
    tokio::time::advance(IDLE - Duration::from_millis(1)).await;
    assert!(futures::poll!(&mut waiting).is_pending());
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(matches!(waiting.await, Next::Idle));
}
