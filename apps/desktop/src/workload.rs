//! Native event-loop measurements using the ordinary Desktop Node owner.
use gpui_kit::*;
use sailry_protocol::{NodeId, Session};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "workload/browser.rs"]
mod browser;
#[path = "workload/driver.rs"]
mod driver;
#[path = "workload/fixture.rs"]
mod fixture;

pub(crate) struct Scenario {
    pub profile: PathBuf,
    output: PathBuf,
    sessions: [(NodeId, Session); 2],
    browser: bool,
    capture: bool,
}

pub(super) fn run(output: &Path) {
    #[cfg(target_os = "macos")]
    if std::env::var("SAILRY_WORKLOAD_CASE")
        .is_ok_and(|value| matches!(value.as_str(), "native-menus" | "native-menu-actions"))
    {
        crate::app_menu::verify_native(output);
        return;
    }
    #[cfg(target_os = "macos")]
    if std::env::var("SAILRY_WORKLOAD_CASE").is_ok_and(|value| value == "file-preview") {
        crate::content::files::verify_native(output);
        return;
    }
    println!("Native frame scheduling: true");
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let case = std::env::var("SAILRY_WORKLOAD_CASE").unwrap_or_default();
    let browser = matches!(case.as_str(), "browser" | "browser-interactions");
    let capture = case == "browser";
    let page = browser.then(|| runtime.block_on(browser::Page::start()));
    let (remote, server, sessions) = runtime.block_on(fixture::prepare(
        directory.path(),
        page.as_ref().map(|page| page.url.clone()),
        capture,
    ));
    crate::run(Scenario {
        profile: directory.path().join("local"),
        output: output.to_owned(),
        sessions,
        browser,
        capture,
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
            if scenario.browser {
                browser::run(scenario, shell, cx).await;
            } else {
                driver::run(scenario, shell, cx).await;
            }
            cx.update(|_, cx| cx.quit()).unwrap();
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
