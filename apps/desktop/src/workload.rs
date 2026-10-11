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
#[path = "workload/check.rs"]
mod check;
#[path = "workload/driver.rs"]
mod driver;
#[path = "workload/fixture.rs"]
mod fixture;

pub(crate) use check::{check, fail, setup};
pub(super) use check::{failed, install_exit};

pub(crate) struct Scenario {
    pub profile: PathBuf,
    output: PathBuf,
    sessions: [(NodeId, Session); 2],
    browser: bool,
    capture: bool,
    document: Option<Arc<crate::agent_fixture::Server>>,
}

pub(super) fn run(output: &Path) {
    #[cfg(target_os = "macos")]
    if std::env::var("SAILRY_WORKLOAD_CASE").is_ok_and(|value| value == "html-brief") {
        crate::plugins::verify_document(output);
        return;
    }
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
    #[cfg(target_os = "macos")]
    if std::env::var("SAILRY_WORKLOAD_CASE").is_ok_and(|value| value == "markdown-visuals") {
        crate::content::visuals::verify_native(output);
        return;
    }
    #[cfg(target_os = "macos")]
    if std::env::var("SAILRY_WORKLOAD_CASE").is_ok_and(|value| value == "conversation-export") {
        crate::live::menus::export::image::check::run(output);
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
    #[cfg(target_os = "macos")]
    if case == "html-brief-flow" {
        let (remote, server, sessions) =
            runtime.block_on(crate::plugins::prepare_document(directory.path()));
        crate::run(Scenario {
            profile: directory.path().join("local"),
            output: output.to_owned(),
            sessions,
            browser: false,
            capture: false,
            document: Some(server),
        });
        runtime.block_on(remote.shutdown()).unwrap();
        return;
    }
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
        document: None,
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
                #[cfg(target_os = "macos")]
                if let Some(server) = scenario.document.clone() {
                    crate::plugins::verify_document_flow(
                        &scenario.output,
                        scenario.sessions,
                        server,
                        shell,
                        cx,
                    )
                    .await;
                    return;
                }
                if scenario.browser {
                    browser::run(scenario, shell, cx).await;
                } else if let Err(error) = driver::run(scenario, shell, cx).await {
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
