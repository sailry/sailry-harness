//! Real WebKit interactions over both Client routes, with a deterministic model endpoint.
#[path = "browser/focus.rs"]
mod focus;

use super::*;
use crate::{
    backend::Services,
    browser::{Browser, state},
    plugins::Panel,
    resources::SideResource,
    shell::{Shell, session_scope::Key},
};
use sailry_client::{Client, conversation::Projection};
use sailry_protocol::{
    Command, Output,
    browser::Action,
    conversation::{Part, Status},
};
use std::cell::RefCell;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub(super) struct Page {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Page {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Page {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut input = [0; 4096];
                    let length = stream.read(&mut input).await.unwrap();
                    let header = String::from_utf8_lossy(&input[..length]);
                    let body = if header.starts_with("GET /second ") {
                        "<html><body>Second page</body></html>"
                    } else {
                        PAGE
                    };
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        Self { url, task }
    }
}
const PAGE: &str = r#"<!doctype html><html><head><title>Browser fixture</title></head><body>
<form id="form"><input aria-label="Message" id="message"><select aria-label="Choice" id="choice"><option value="a">First</option><option value="b">Second</option></select></form>
<button aria-label="Hover" id="hover">Hover</button><p id="status">Pending</p>
<iframe title="Same origin" srcdoc="<html><body><input aria-label='Frame input' oninput='document.querySelector(&quot;output&quot;).textContent = this.value'><output></output></body></html>"></iframe>
<iframe title="Cross origin" sandbox srcdoc="<html><body>Opaque frame</body></html>"></iframe>
<a href="/second">Second page</a>
<script>
let hovered = false;
hover.addEventListener('mouseenter', () => { hovered = true; });
form.addEventListener('submit', event => { event.preventDefault(); document.querySelector('#status').textContent = `Submitted ${message.value} ${choice.value} ${hovered ? 'hovered' : 'not hovered'}`; });
setTimeout(() => { const ready = document.createElement('p'); ready.id = 'ready'; ready.textContent = 'Ready'; document.body.append(ready); }, 250);
</script></body></html>"#;

pub(super) async fn run(scenario: Scenario, shell: Entity<Shell>, cx: &mut AsyncWindowContext) {
    let services = cx
        .read_global::<Services, _>(|services, _, _| services.clone())
        .unwrap();
    let capture_enabled = scenario.capture;
    for (route, (node, session)) in ["local", "iroh"].into_iter().zip(scenario.sessions) {
        until(cx, "known host", |_, cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&node)
        })
        .await;
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node, cx)
            })
        })
        .unwrap();
        until(cx, "connected host", |_, cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        })
        .await;
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(session.clone(), window, cx)
            })
        })
        .unwrap();
        until(cx, "browser subscription", |_, cx| {
            shell.read(cx).browsers.connected(node)
        })
        .await;
        let client = Arc::new(
            cx.update(|_, cx| Client::new(shell.read(cx).live.as_ref().unwrap().transport.clone()))
                .unwrap(),
        );
        let execution = client.clone();
        let turn_session = session.clone();
        let results = services
            .runtime
            .spawn(async move {
                let client = execution;
                let session = turn_session;
                let mut subscription = client.subscribe_conversation(session.id).await.unwrap();
                let mut projection = Projection::new(client.target(), session.id, 1);
                projection
                    .apply(1, subscription.next().await.unwrap())
                    .unwrap();
                let Output::QueuedTurn(turn) = fixture::execute(
                    &client,
                    Command::SubmitTurn {
                        session: session.id,
                        expected_revision: session.revision,
                        message: "Exercise the isolated browser fixture".into(),
                    },
                )
                .await
                else {
                    panic!("turn expected")
                };
                tokio::time::timeout(Duration::from_secs(90), async {
                    loop {
                        projection
                            .apply(1, subscription.next().await.unwrap())
                            .unwrap();
                        let page = &projection.snapshot().unwrap().page;
                        if let Some(run) = page.runs.iter().find(|run| run.turn == turn.id)
                            && !matches!(
                                run.status,
                                Status::Queued | Status::Running | Status::Stopping
                            )
                        {
                            assert_eq!(run.status, Status::Completed, "{route}: {:?}", run.error);
                            let results: Vec<_> = page
                                .entries
                                .iter()
                                .filter(|entry| entry.turn == turn.id)
                                .flat_map(|entry| &entry.parts)
                                .filter_map(|part| {
                                    if let Part::ToolResult { result, .. } = part {
                                        Some(result.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            verify(&results, capture_enabled);
                            if capture_enabled {
                                download(&client, &session, &results[24]).await;
                            }
                            return results;
                        }
                    }
                })
                .await
                .expect("browser workflow deadline")
            })
            .await
            .unwrap();
        background(
            &client,
            &services,
            &session,
            results[0]["url"].as_str().unwrap(),
            &shell,
            cx,
        )
        .await;
        println!(
            "Browser workflow {route}: {} real WebView tool results and background tab interactions verified",
            results.len()
        );
    }
    std::fs::write(
        scenario.output.join("browser.json"),
        serde_json::json!({"local": true, "remote": true, "model": "fixture",
            "screenshots": "captured and downloaded", "foreground_capture": capture_enabled,
            "background": {"sidebar_preserved": true, "selection_preserved": true,
                "read_click_navigation": true, "selection_promotes": true, "focus_promotes": true,
                "native_focus_preserved": true, "background_close": true}
        })
        .to_string(),
    )
    .unwrap();
}

async fn background(
    client: &Arc<Client>,
    services: &Services,
    session: &Session,
    url: &str,
    shell: &Entity<Shell>,
    cx: &mut AsyncWindowContext,
) {
    let browser = cx
        .update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.browser_for(Key::Session(client.target(), session.id), window, cx)
            })
        })
        .unwrap();
    until(cx, "browser panel", |_, cx| {
        matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel))
            if panel.read(cx).browser_view().as_ref() == Some(&browser)
                && panel.read(cx).resource_active())
    })
    .await;
    let (panel, selected) = cx
        .update(|_, cx| {
            let Some(SideResource::Plugin(panel)) = &shell.read(cx).side_resource else {
                panic!("browser panel expected")
            };
            (
                panel.clone(),
                browser.read(cx).snapshot(cx)["selected"].as_u64().unwrap(),
            )
        })
        .unwrap();
    let native_focus = focus::State::foreground(&browser, selected, cx).await;
    let opened = call(
        client,
        services,
        session,
        Action::Open {
            url: url.into(),
            background: true,
        },
    )
    .await;
    let hidden = opened["tab"].as_u64().unwrap();
    assert_ne!(hidden, selected);
    assert_eq!(
        tab(
            &retained(shell, &browser, Some(&panel), selected, cx),
            hidden
        )["background"],
        true
    );
    native_focus.check(&browser, Some(hidden), "background open", cx);
    let read = call(
        client,
        services,
        session,
        Action::Read { tab: Some(hidden) },
    )
    .await;
    assert!(read["text"].as_str().unwrap().contains("Pending"));
    let link = read["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|element| element["label"] == "Second page")
        .expect("background page link");
    retained(shell, &browser, Some(&panel), selected, cx);
    native_focus.check(&browser, Some(hidden), "background read", cx);
    let clicked = call(
        client,
        services,
        session,
        Action::Click {
            tab: hidden,
            snapshot: read["snapshot"].as_str().unwrap().into(),
            element: link["id"].as_u64().unwrap(),
        },
    )
    .await;
    assert!(clicked["url"].as_str().unwrap().ends_with("/second"));
    assert!(clicked["text"].as_str().unwrap().contains("Second page"));
    assert_eq!(
        tab(
            &retained(shell, &browser, Some(&panel), selected, cx),
            hidden
        )["background"],
        true
    );
    native_focus.check(&browser, Some(hidden), "background click", cx);
    let navigated = call(
        client,
        services,
        session,
        Action::Navigate {
            url: url.into(),
            tab: Some(hidden),
            background: true,
        },
    )
    .await;
    assert_eq!(navigated["url"], url);
    assert!(navigated["text"].as_str().unwrap().contains("Pending"));
    retained(shell, &browser, Some(&panel), selected, cx);
    native_focus.check(&browser, Some(hidden), "background navigation", cx);
    let capture = call(
        client,
        services,
        session,
        Action::Screenshot { tab: hidden },
    )
    .await;
    assert_eq!(capture["tab"], hidden);
    let download_client = client.clone();
    let download_session = session.clone();
    services
        .runtime
        .spawn(async move {
            download(&download_client, &download_session, &capture).await;
        })
        .await
        .unwrap();
    retained(shell, &browser, Some(&panel), selected, cx);
    native_focus.check(&browser, Some(hidden), "background capture", cx);
    let tabs = call(client, services, session, Action::Tabs).await;
    assert_eq!(tab(&tabs, hidden)["background"], true);
    assert_eq!(tab(&tabs, selected)["background"], false);
    retained(shell, &browser, Some(&panel), selected, cx);
    native_focus.check(&browser, Some(hidden), "background tabs", cx);

    let closed = call(client, services, session, Action::Close { tab: hidden }).await;
    assert_eq!(closed["closed"], hidden);
    retained(shell, &browser, Some(&panel), selected, cx);
    native_focus.check(&browser, None, "background close", cx);
    let opened = call(
        client,
        services,
        session,
        Action::Open {
            url: url.into(),
            background: true,
        },
    )
    .await;
    let hidden = opened["tab"].as_u64().unwrap();
    retained(shell, &browser, Some(&panel), selected, cx);
    native_focus.check(&browser, Some(hidden), "replacement background open", cx);

    let (reply, receive) = tokio::sync::oneshot::channel();
    cx.update(|_, cx| {
        browser.update(cx, |_, cx| {
            cx.emit(state::Request {
                action: state::Action::Select {
                    id: usize::try_from(hidden).unwrap(),
                },
                stop: sailry_link::CancellationToken::new(),
                reply: RefCell::new(Some(reply)),
            });
        });
    })
    .unwrap();
    let promoted = receive.await.unwrap().unwrap();
    assert_eq!(promoted["selected"], hidden);
    assert_eq!(tab(&promoted, hidden)["background"], false);
    retained(shell, &browser, Some(&panel), hidden, cx);

    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.close_resource_panel(cx));
    })
    .unwrap();
    let composer_focus = focus::State::composer(shell, &browser, hidden, cx);
    let opened = call(
        client,
        services,
        session,
        Action::Open {
            url: url.into(),
            background: true,
        },
    )
    .await;
    let focus = opened["tab"].as_u64().unwrap();
    assert_eq!(
        tab(&retained(shell, &browser, None, hidden, cx), focus)["background"],
        true
    );
    composer_focus.check(&browser, Some(focus), "closed-sidebar background open", cx);
    let read = call(client, services, session, Action::Read { tab: Some(focus) }).await;
    assert!(read["text"].as_str().unwrap().contains("Pending"));
    retained(shell, &browser, None, hidden, cx);
    composer_focus.check(&browser, Some(focus), "closed-sidebar background read", cx);
    let link = read["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|element| element["label"] == "Second page")
        .expect("background page link");
    let clicked = call(
        client,
        services,
        session,
        Action::Click {
            tab: focus,
            snapshot: read["snapshot"].as_str().unwrap().into(),
            element: link["id"].as_u64().unwrap(),
        },
    )
    .await;
    assert!(clicked["text"].as_str().unwrap().contains("Second page"));
    retained(shell, &browser, None, hidden, cx);
    composer_focus.check(&browser, Some(focus), "closed-sidebar background click", cx);
    let capture = call(client, services, session, Action::Screenshot { tab: focus }).await;
    assert_eq!(capture["tab"], focus);
    let download_client = client.clone();
    let download_session = session.clone();
    services
        .runtime
        .spawn(async move {
            download(&download_client, &download_session, &capture).await;
        })
        .await
        .unwrap();
    retained(shell, &browser, None, hidden, cx);
    composer_focus.check(
        &browser,
        Some(focus),
        "closed-sidebar background capture",
        cx,
    );
    call(
        client,
        services,
        session,
        Action::Navigate {
            url: format!("{url}second"),
            tab: Some(focus),
            background: true,
        },
    )
    .await;
    retained(shell, &browser, None, hidden, cx);
    composer_focus.check(
        &browser,
        Some(focus),
        "closed-sidebar background navigation",
        cx,
    );
    let closed = call(client, services, session, Action::Close { tab: focus }).await;
    assert_eq!(closed["closed"], focus);
    retained(shell, &browser, None, hidden, cx);
    composer_focus.check(&browser, None, "closed-sidebar background close", cx);
    let opened = call(
        client,
        services,
        session,
        Action::Open {
            url: format!("{url}second"),
            background: true,
        },
    )
    .await;
    let focus = opened["tab"].as_u64().unwrap();
    retained(shell, &browser, None, hidden, cx);
    composer_focus.check(&browser, Some(focus), "closed-sidebar replacement open", cx);
    let focused = call(client, services, session, Action::Focus { tab: focus }).await;
    assert_eq!(focused["tab"], focus);
    assert!(focused["text"].as_str().unwrap().contains("Second page"));
    until(cx, "focused browser panel", |_, cx| {
        let shell = shell.read(cx);
        shell.layout.panel_open[0]
            && matches!(&shell.side_resource, Some(SideResource::Plugin(panel))
                if panel.read(cx).browser_view().as_ref() == Some(&browser)
                    && panel.read(cx).resource_active())
    })
    .await;
    cx.update(|_, cx| {
        let state = browser.read(cx).snapshot(cx);
        assert_eq!(state["selected"], focus);
        assert_eq!(tab(&state, focus)["background"], false);
    })
    .unwrap();
}

fn retained(
    shell: &Entity<Shell>,
    browser: &Entity<Browser>,
    panel: Option<&Entity<Panel>>,
    selected: u64,
    cx: &mut AsyncWindowContext,
) -> serde_json::Value {
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.layout.panel_open[0], panel.is_some());
        match (&shell.side_resource, panel) {
            (Some(SideResource::Plugin(current)), Some(expected)) => assert_eq!(current, expected),
            (None, None) => {}
            _ => panic!("background operation replaced the resource panel"),
        }
        let state = browser.read(cx).snapshot(cx);
        assert_eq!(
            state["selected"], selected,
            "background operation selected another tab"
        );
        state
    })
    .unwrap()
}

fn tab(state: &serde_json::Value, id: u64) -> &serde_json::Value {
    state["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tab| tab["id"].as_u64() == Some(id))
        .expect("retained browser tab")
}

async fn call(
    client: &Arc<Client>,
    services: &Services,
    session: &Session,
    action: Action,
) -> serde_json::Value {
    let client = client.clone();
    let command = Command::UseBrowser {
        session: session.id,
        worktree: session.worktree,
        action,
    };
    let Output::Browser(value) = services
        .runtime
        .spawn(async move { fixture::execute(&client, command).await })
        .await
        .unwrap()
    else {
        panic!("browser result expected")
    };
    value
}

async fn download(client: &Client, session: &Session, capture: &serde_json::Value) {
    let Output::FileDownload(download) = fixture::execute(
        client,
        Command::DownloadFile {
            worktree: session.worktree,
            path: capture["path"].as_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("capture download expected")
    };
    let mut bytes = Vec::new();
    client
        .download(
            &download,
            &mut bytes,
            sailry_link::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(
        image::guess_format(&bytes).unwrap(),
        image::ImageFormat::Jpeg
    );
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!(Some(u64::from(image.width())), capture["width"].as_u64());
    assert_eq!(Some(u64::from(image.height())), capture["height"].as_u64());
    assert!(image.width() > 0 && image.height() > 0 && bytes.len() <= 180 * 1024);
    assert!(capture.get("base64_image").is_none());
}

fn verify(results: &[serde_json::Value], capture: bool) {
    assert_eq!(results.len(), if capture { 25 } else { 24 });
    for (index, value) in results.iter().enumerate() {
        if [7, 19, 20, 22].contains(&index) {
            assert!(value.get("error").is_some(), "{index}: {value}");
        } else {
            assert!(value.get("error").is_none(), "{index}: {value}");
        }
    }
    assert!(
        results[6]["text"]
            .as_str()
            .unwrap()
            .contains("Submitted Browser fixture b hovered")
    );
    assert_eq!(results[8]["in_frame"], true);
    assert!(results[9]["text"].as_str().unwrap().contains("Frame value"));
    assert_eq!(results[10]["in_frame"], false);
    assert!(results[11]["url"].as_str().unwrap().ends_with("/second"));
    assert!(!results[12]["url"].as_str().unwrap().ends_with("/second"));
    assert!(results[13]["url"].as_str().unwrap().ends_with("/second"));
    assert_eq!(results[14]["url"], results[13]["url"]);
    assert_ne!(results[15]["tab"], results[0]["tab"]);
    assert_eq!(results[16]["tab"], results[0]["tab"]);
    assert_eq!(results[17]["closed"], results[15]["tab"]);
    assert!(
        results[7]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Page changed")
    );
    assert!(
        results[19]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    assert!(
        results[22]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cross-origin")
    );
}
