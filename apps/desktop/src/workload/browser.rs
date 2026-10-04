//! Real WebKit interactions over both Client routes, with a deterministic model endpoint.
use super::*;
use crate::{backend::Services, shell::Shell};
use sailry_client::{Client, conversation::Projection};
use sailry_protocol::{
    Command, Output,
    conversation::{Part, Status},
};
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
        let client = cx
            .update(|_, cx| Client::new(shell.read(cx).live.as_ref().unwrap().transport.clone()))
            .unwrap();
        let result = services
            .runtime
            .spawn(async move {
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
                            if !capture_enabled {
                                return results.len();
                            }
                            let capture = &results[24];
                            let Output::FileDownload(download) = fixture::execute(
                                &client,
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
                            assert!(
                                image.width() > 0
                                    && image.height() > 0
                                    && bytes.len() <= 180 * 1024
                            );
                            assert!(capture.get("base64_image").is_none());
                            return results.len();
                        }
                    }
                })
                .await
                .expect("browser workflow deadline")
            })
            .await
            .unwrap();
        println!("Browser workflow {route}: {result} real WebView tool results verified");
    }
    std::fs::write(
        scenario.output.join("browser.json"),
        serde_json::json!({"local": true, "remote": true, "model": "fixture",
            "screenshots": if capture_enabled { "captured and downloaded" } else { "not run" }
        })
        .to_string(),
    )
    .unwrap();
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
