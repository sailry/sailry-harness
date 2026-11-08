use super::*;
use sailry_protocol::process::{Access, Completion, Outcome, OutputStream};

struct Fixture {
    _directory: tempfile::TempDir,
    commands: Arc<Commands>,
    retained: Arc<crate::process::output::Retained>,
    output: watch::Sender<(Capture, Capture)>,
    session: SessionId,
    id: RequestId,
    closed: CancellationToken,
}

impl Fixture {
    fn new(promoted: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let commands = Arc::new(Commands::default());
        let session = SessionId::new();
        let id = RequestId::new();
        let retained = commands.retain(
            id,
            session,
            TurnId::new(),
            &Launch {
                root: directory.path().into(),
                profile: None,
                command: "fixture".into(),
                access: Access::Unrestricted,
                pty: false,
                cwd: String::new(),
                timeout_ms: 120_000,
                inputs: None,
                scratch: None,
                python: None,
            },
        );
        let (output, received) = watch::channel((Capture::default(), Capture::default()));
        {
            let mut entries = commands.entries.lock().unwrap();
            let entry = entries.get_mut(&id).unwrap();
            entry.output = received;
            entry.promoted = promoted;
        }
        Self {
            _directory: directory,
            commands,
            retained,
            output,
            session,
            id,
            closed: CancellationToken::new(),
        }
    }

    fn wait(
        &self,
        wait_ms: u64,
        until: Option<&str>,
    ) -> sailry_link::Pending<'static, Result<(), Fault>> {
        self.commands.wait(
            self.session,
            self.id,
            wait_ms,
            until.map(str::to_owned),
            self.closed.clone(),
        )
    }

    fn append(&self, text: &str) {
        self.retained.append(OutputStream::Stdout, text.as_bytes());
        self.output.send_modify(|(stdout, _)| {
            stdout.text.push_str(text);
        });
    }

    fn finish(&self) {
        self.commands.finish(
            self.id,
            Ok(&Completion {
                outcome: Outcome::Exited(0),
                stdout: self.output.borrow().0.clone(),
                stderr: Capture::default(),
                elapsed_ms: 0,
            }),
        );
    }
}

#[tokio::test(start_paused = true)]
async fn wakes_on_new_output() {
    for promoted in [false, true] {
        let fixture = Fixture::new(promoted);
        fixture.append("before");
        let mut waiting = Box::pin(fixture.wait(30_000, None));
        assert!(futures::poll!(waiting.as_mut()).is_pending());
        if promoted {
            fixture.retained.append(OutputStream::Stdout, b"after");
        } else {
            fixture.append("after");
        }
        waiting.await.unwrap();
        assert_eq!(
            fixture
                .commands
                .read(fixture.session, fixture.id)
                .unwrap()
                .stdout
                .text,
            "beforeafter"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn retains_output_before_first_poll() {
    let fixture = Fixture::new(false);
    let waiting = fixture.wait(30_000, None);
    fixture.append("arrived after admission");
    let started = tokio::time::Instant::now();
    waiting.await.unwrap();
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn joins_split_substring() {
    let fixture = Fixture::new(false);
    let mut waiting = Box::pin(fixture.wait(30_000, Some("ready")));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    fixture.append("rea");
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    fixture.append("dy");
    waiting.await.unwrap();
    fixture.wait(30_000, Some("ready")).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn empty_text_waits_for_output() {
    for promoted in [false, true] {
        let fixture = Fixture::new(promoted);
        fixture.append("before");
        let mut waiting = Box::pin(fixture.wait(30_000, Some("")));
        assert!(futures::poll!(waiting.as_mut()).is_pending());
        fixture.append("after");
        waiting.await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn wakes_on_exit_without_output() {
    let fixture = Fixture::new(false);
    let mut waiting = Box::pin(fixture.wait(30_000, Some("absent")));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    fixture.finish();
    waiting.await.unwrap();
    fixture.wait(30_000, None).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn bounds_wait_and_honors_shutdown() {
    let fixture = Fixture::new(false);
    for (requested, expected) in [(0, 0), (25, 25), (u64::MAX, 30_000)] {
        let started = tokio::time::Instant::now();
        fixture.wait(requested, None).await.unwrap();
        assert_eq!(started.elapsed().as_millis(), expected);
    }
    let mut waiting = Box::pin(fixture.wait(30_000, None));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    fixture.closed.cancel();
    assert_eq!(waiting.await.unwrap_err().code, ErrorCode::Unavailable);
}

#[tokio::test]
async fn keeps_session_ownership() {
    let fixture = Fixture::new(false);
    assert_eq!(
        fixture
            .commands
            .wait(
                SessionId::new(),
                fixture.id,
                30_000,
                None,
                fixture.closed.clone()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
