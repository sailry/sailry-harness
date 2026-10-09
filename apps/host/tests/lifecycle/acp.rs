use super::*;
use super::{tasks::fixture::Fixture, workload::models};
use sailry_link::CancellationToken;
use sailry_protocol::{Command as NodeCommand, Output, RequestId, conversation::Status};
use serde_json::{Value, json};
use std::io::Write;

struct Editor {
    child: Child,
    input: Option<std::process::ChildStdin>,
    output: tokio::sync::mpsc::UnboundedReceiver<Value>,
    updates: Vec<Value>,
}

impl Editor {
    fn new(profile: &Path) -> Self {
        let binary = std::env::var_os("SAILRY_TEST_HOST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into());
        let mut child = Command::new(binary)
            .arg("acp")
            .arg("--data-dir")
            .arg(profile)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let frame = serde_json::from_str(&line.unwrap()).unwrap();
                if sender.send(frame).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input: Some(input),
            output,
            updates: Vec::new(),
        }
    }

    fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }

    async fn next(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(20), self.output.recv())
            .await
            .unwrap()
            .unwrap()
    }

    async fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        self.reply(id).await
    }

    async fn open(&mut self, fixture: &Fixture) {
        let initialized = self
            .request(
                1,
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":{}}),
            )
            .await;
        assert_eq!(initialized["result"]["protocolVersion"], 1, "{initialized}");
        let loaded = self
            .request(
                2,
                "session/load",
                json!({"sessionId":fixture.session.id,"cwd":fixture.root,"mcpServers":[]}),
            )
            .await;
        assert!(loaded.get("result").is_some(), "{loaded}");
    }

    async fn admitted(&mut self) {
        loop {
            let frame = self.next().await;
            assert_eq!(frame["method"], "session/update", "{frame}");
            let update = frame["params"].clone();
            let admitted = update["_meta"]["sailry"]["state"] == "admitted";
            self.updates.push(update);
            if admitted {
                return;
            }
        }
    }

    async fn reply(&mut self, id: u64) -> Value {
        loop {
            let frame = self.next().await;
            if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
                return frame;
            }
            assert_eq!(frame["method"], "session/update", "{frame}");
            self.updates.push(frame["params"].clone());
        }
    }

    async fn close(mut self) {
        drop(self.input.take());
        let output = tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if self.child.try_wait().unwrap().is_some() {
                    return self.child.wait_with_output().unwrap();
                }
                if Instant::now() > deadline {
                    self.child.kill().unwrap();
                    panic!("ACP process did not close its observation");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_reuses_running_host_and_history() {
    let fixture = Fixture::new(models::Server::markdown("Actual stdio 中文 🙂".into()).await).await;
    let mut editor = Editor::new(&fixture.profile);
    editor.open(&fixture).await;
    let request = RequestId::new();
    let completed = editor.request(3, "session/prompt", json!({"sessionId":fixture.session.id,
        "prompt":[{"type":"text","text":"Use the existing Node"}],"_meta":{"sailry":{"request_id":request,"expected_revision":fixture.session.revision}}})).await;
    assert_eq!(completed["result"]["stopReason"], "end_turn");
    assert_eq!(
        completed["result"]["_meta"]["sailry"]["node"],
        json!(fixture.client.target())
    );
    assert_eq!(
        completed["result"]["_meta"]["sailry"]["request_id"],
        json!(request)
    );
    let text: String = editor
        .updates
        .iter()
        .filter(|update| update["update"]["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|update| update["update"]["content"]["text"].as_str())
        .collect();
    assert_eq!(text, "Actual stdio 中文 🙂");
    assert_eq!(fixture.page().await.runs.len(), 1);
    assert_eq!(fixture.page().await.runs[0].status, Status::Completed);
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
    assert!(Node::start(&fixture.profile).await.is_err());
    editor.close().await;
    let Output::Session(session) = fixture
        .execute(NodeCommand::ReadSession {
            session: fixture.session.id,
        })
        .await
    else {
        panic!("session expected")
    };
    assert_eq!(session.config, fixture.session.config);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_eof_keeps_admitted_work_running() {
    let release = CancellationToken::new();
    let fixture = Fixture::new(
        models::Server::markdown_held("Completes after EOF".into(), release.clone()).await,
    )
    .await;
    let mut editor = Editor::new(&fixture.profile);
    editor.open(&fixture).await;
    editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":fixture.session.id,
        "prompt":[{"type":"text","text":"Keep working through disconnection"}]}}));
    editor.admitted().await;
    editor.close().await;
    assert_eq!(fixture.page().await.runs[0].status, Status::Running);
    release.cancel();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if fixture.page().await.runs[0].status == Status::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_cancel_stops_the_native_turn() {
    let fixture = Fixture::new(
        models::Server::markdown_held("Held answer".into(), CancellationToken::new()).await,
    )
    .await;
    let mut editor = Editor::new(&fixture.profile);
    editor.open(&fixture).await;
    let request = RequestId::new();
    editor.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":fixture.session.id,
        "prompt":[{"type":"text","text":"Wait until explicitly stopped"}],"_meta":{"sailry":{"request_id":request}}}}));
    editor.admitted().await;
    editor.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":fixture.session.id}}));
    let stopped = editor.reply(3).await;
    assert_eq!(stopped["result"]["stopReason"], "cancelled");
    assert_eq!(
        stopped["result"]["_meta"]["sailry"]["request_id"],
        json!(request)
    );
    assert_eq!(fixture.page().await.runs[0].status, Status::Cancelled);
    assert_eq!(fixture.page().await.runs.len(), 1);
    editor.close().await;
    fixture.close().await;
}

#[test]
fn requires_existing_profile_owner() {
    let directory = tempdir().unwrap();
    let profile = directory.path().join("absent");
    let binary = std::env::var_os("SAILRY_TEST_HOST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into());
    let output = Command::new(binary)
        .arg("acp")
        .arg("--data-dir")
        .arg(&profile)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Host is not running"));
    assert!(!profile.exists());
}
