//! Native event-loop measurements using the ordinary Desktop Node owner.
use gpui_kit::*;
use sailry_protocol::{NodeId, Session};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "workload/check.rs"]
mod check;
#[path = "workload/driver.rs"]
mod driver;
#[path = "workload/fixture.rs"]
mod fixture;
#[path = "workload/frames.rs"]
mod frames;

pub(crate) use check::{check, fail, setup};
pub(super) use check::{failed, install_exit};

pub(crate) struct Scenario {
    pub profile: PathBuf,
    output: PathBuf,
    sessions: [(NodeId, Session); 2],
}

pub(super) fn run(output: &Path) {
    println!("Native frame scheduling: true");
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let (remote, server, sessions) = runtime.block_on(fixture::prepare(directory.path()));
    crate::run(Scenario {
        profile: directory.path().join("local"),
        output: output.to_owned(),
        sessions,
    });
    runtime.block_on(remote.shutdown()).unwrap();
    drop(server);
}

pub(crate) fn attach(
    scenario: Scenario,
    shell: Entity<crate::shell::Shell>,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            let _ = check(async {
                if let Err(error) = driver::run(scenario, shell, cx).await {
                    fail(error);
                }
            })
            .await;
            cx.update(|_, cx| cx.quit()).ok();
        })
        .detach();
}

async fn pause(cx: &AsyncWindowContext, duration: Duration) {
    cx.background_executor().timer(duration).await;
}

async fn until(
    cx: &mut AsyncWindowContext,
    state: &str,
    mut ready: impl FnMut(&mut Window, &mut App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if cx.update(&mut ready).unwrap() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native workload state deadline: {state}"
        );
        pause(cx, Duration::from_millis(20)).await;
    }
}
