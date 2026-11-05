use super::*;
use crate::process::{
    Access, Environment, Launch,
    environment::{EnvironmentFixture, Source},
    pty,
};
use sailry_link::CancellationToken;
use sailry_protocol::process::{Capture, Completion, Outcome};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

const FIXTURE: &str = "process::interactive::windows::tests::fixture";

struct Running {
    task: Option<JoinHandle<Result<Completion, Fault>>>,
    input: mpsc::Sender<Vec<u8>>,
    output: watch::Receiver<(Capture, Capture)>,
    stop: CancellationToken,
    closed: CancellationToken,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Running {
    async fn start(root: &Path, case: &str) -> Self {
        Self::command(root, case, "fixture.cmd").await
    }

    async fn command(root: &Path, case: &str, script: &str) -> Self {
        let exe = batch(&std::env::current_exe().unwrap());
        let path = batch(root);
        std::fs::write(
            root.join("fixture.cmd"),
            format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\nchcp 65001 >nul\r\nset \"SAILRY_PTY_CASE={case}\"\r\nset \"SAILRY_PTY_ROOT={path}\"\r\n\"{exe}\" --exact {FIXTURE} --nocapture\r\n"
            ),
        )
        .unwrap();
        let values = environment(root);
        let environment = Environment::capture(Source::Fixture(EnvironmentFixture {
            home: root.join("home"),
            shell: PathBuf::from(&values[&OsString::from("COMSPEC")]),
            environment: values,
            timeout: Duration::from_secs(1),
        }))
        .await;
        let mut launch = launch(root);
        launch.command = script.into();
        let stop = CancellationToken::new();
        let closed = CancellationToken::new();
        let (progress, output) = watch::channel((Capture::default(), Capture::default()));
        let (input, received) = mpsc::channel(32);
        let stopping = stop.clone();
        let closing = closed.clone();
        let retained = Arc::new(crate::process::output::Retained::new(root, Arc::default()));
        let task = tokio::spawn(async move {
            super::super::execute(
                launch,
                &environment,
                &stopping,
                &closing,
                &progress,
                received,
                retained,
            )
            .await
        });
        Self {
            task: Some(task),
            input,
            output,
            stop,
            closed,
        }
    }

    async fn ready(&mut self) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !self.output.borrow().0.text.contains("fixture-ready") {
                self.output.changed().await.unwrap_or_else(|error| {
                    panic!(
                        "PTY output ended before readiness: {error}; output: {:?}",
                        self.output.borrow().0
                    )
                });
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "PTY readiness deadline: {error}; output: {:?}",
                self.output.borrow().0
            )
        });
    }

    async fn finish(mut self) -> Completion {
        tokio::time::timeout(Duration::from_secs(20), self.task.take().unwrap())
            .await
            .expect("PTY completion deadline")
            .unwrap()
            .unwrap()
    }
}

fn launch(root: &Path) -> Launch {
    Launch {
        root: root.canonicalize().unwrap(),
        profile: None,
        command: "fixture.cmd".into(),
        access: Access::Unrestricted,
        pty: true,
        cwd: String::new(),
        timeout_ms: 1,
        inputs: None,
        scratch: None,
        python: None,
    }
}

fn environment(root: &Path) -> BTreeMap<OsString, OsString> {
    let system = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows system root"));
    let bin = system.join("System32");
    let shell = bin.join("cmd.exe");
    assert!(shell.is_file(), "Windows command interpreter missing");
    std::fs::create_dir_all(root.join("home")).unwrap();
    BTreeMap::from([
        ("SystemRoot".into(), system.into_os_string()),
        ("COMSPEC".into(), shell.into_os_string()),
        ("PATH".into(), bin.into_os_string()),
        ("PATHEXT".into(), ".EXE;.CMD".into()),
        ("HOME".into(), root.join("home").into_os_string()),
        ("USERPROFILE".into(), root.join("home").into_os_string()),
        ("TEMP".into(), root.as_os_str().to_owned()),
        ("TMP".into(), root.as_os_str().to_owned()),
    ])
}

fn batch(path: &Path) -> String {
    let text = path.to_str().unwrap();
    assert!(!text.contains(['"', '\r', '\n']));
    text.replace('%', "%%")
}

fn child(root: &Path) -> OwnedHandle {
    let pid = std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .parse()
        .unwrap();
    // SAFETY: the fixture wrote this PID before readiness; only synchronization
    // access is requested and this retained handle identifies the same process.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!handle.is_null(), "fixture child handle missing");
    // SAFETY: OpenProcess returned a new handle owned by this test only.
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

async fn exited(handle: &OwnedHandle) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // SAFETY: the retained process handle remains live; a zero timeout
            // only inspects termination and does not mutate the process.
            match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => return,
                WAIT_TIMEOUT => tokio::time::sleep(Duration::from_millis(10)).await,
                result => panic!("fixture wait failed: {result}"),
            }
        }
    })
    .await
    .expect("PTY descendant did not exit");
}

#[test]
fn prepares_working_directories() {
    for (source, expected) in [
        (r"\\?\C:\project", r"C:\project"),
        (r"\\?\C:\项目 with spaces", r"C:\项目 with spaces"),
        (r"\\?\C:\", r"C:\"),
        (r"C:\project", r"C:\project"),
        (r"\\server\share\project", r"\\server\share\project"),
        (r"\\?\UNC\server\share", r"\\?\UNC\server\share"),
        (r"\\.\pipe\fixture", r"\\.\pipe\fixture"),
        (r"\\?\C:\project.", r"\\?\C:\project."),
        (r"\\?\C:\project ", r"\\?\C:\project "),
    ] {
        assert_eq!(super::directory(Path::new(source)), Path::new(expected));
    }
}

#[tokio::test]
async fn honors_working_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("项目 with spaces");
    std::fs::create_dir(&root).unwrap();
    let script = r#"if not "%CD%"=="%PWD%" exit /b 9 & echo cwd-marker>cwd.txt"#;
    let result = Running::command(&root, "tail", script).await.finish().await;
    assert_eq!(result.outcome, Outcome::Exited(0), "{:?}", result.stdout);
    assert_eq!(
        std::fs::read_to_string(root.join("cwd.txt"))
            .unwrap()
            .trim(),
        "cwd-marker"
    );
}

#[tokio::test]
async fn closes_after_draining_unicode_output() {
    let directory = tempfile::tempdir().unwrap();
    let running = Running::start(directory.path(), "tail").await;
    let result = running.finish().await;
    assert_eq!(result.outcome, Outcome::Exited(7), "{:?}", result.stdout);
    assert!(
        result.stdout.text.contains("中文 🙂 final tail"),
        "{:?}",
        result.stdout
    );
    assert!(result.stdout.text.len() <= 64 * 1024);
    assert!(!result.stdout.invalid_utf8);
    assert!(!result.stdout.text.contains("\u{1b}[6n"));
    assert_eq!(result.stderr, Capture::default());
}

#[tokio::test]
async fn preserves_cmd_quoting() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("项目 with spaces");
    std::fs::create_dir(&root).unwrap();
    for (script, expected) in [
        (r#"echo "quoted & value""#, "\"quoted & value\""),
        (r#"for %G in ("quoted value") do @echo %~G"#, "quoted value"),
        (r#"echo "pipe value" | findstr "pipe""#, "\"pipe value\""),
        (r#"echo escaped ^& value"#, "escaped & value"),
    ] {
        let result = Running::command(&root, "tail", script).await.finish().await;
        assert_eq!(result.outcome, Outcome::Exited(0), "{script}");
        assert!(
            result.stdout.text.contains(expected),
            "{script}: {:?}",
            result.stdout
        );
        assert!(!result.stdout.invalid_utf8);
        assert_eq!(result.stderr, Capture::default());
    }
}

#[tokio::test]
async fn starts_quoted_script_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("项目 with spaces");
    std::fs::create_dir(&root).unwrap();
    let script = format!("\"{}\"", root.join("fixture.cmd").display());
    let result = Running::command(&root, "tail", &script)
        .await
        .finish()
        .await;
    assert_eq!(result.outcome, Outcome::Exited(7));
    assert!(result.stdout.text.contains("中文 🙂 final tail"));
    assert!(!result.stdout.invalid_utf8);
}

#[tokio::test]
async fn accepts_unicode_input() {
    let directory = tempfile::tempdir().unwrap();
    let mut running = Running::start(directory.path(), "reply").await;
    running.ready().await;
    running
        .input
        .send("quoted \"中文 🙂\" & value\r".as_bytes().to_vec())
        .await
        .unwrap();
    let result = running.finish().await;
    assert_eq!(result.outcome, Outcome::Exited(7));
    assert!(
        result
            .stdout
            .text
            .contains("input:quoted \"中文 🙂\" & value")
    );
    assert!(!result.stdout.invalid_utf8);
    assert_eq!(result.stderr, Capture::default());
}

#[tokio::test]
async fn root_exit_closes_descendants() {
    let directory = tempfile::tempdir().unwrap();
    let mut running = Running::start(directory.path(), "exit").await;
    running.ready().await;
    let child = child(directory.path());
    running.input.send(b"exit\r".to_vec()).await.unwrap();
    assert_eq!(running.finish().await.outcome, Outcome::Exited(7));
    exited(&child).await;
}

#[tokio::test]
async fn cancellation_closes_the_job_and_output() {
    for closing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut running = Running::start(directory.path(), "idle").await;
        running.ready().await;
        let child = child(directory.path());
        let input = running.input.clone();
        let writing = tokio::spawn(async move {
            for _ in 0..80 {
                if input.send(vec![b'x'; 64 * 1024]).await.is_err() {
                    return;
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!running.task.as_ref().unwrap().is_finished());
        if closing {
            running.closed.cancel();
        } else {
            running.stop.cancel();
        }
        assert_eq!(running.finish().await.outcome, Outcome::Cancelled);
        exited(&child).await;
        tokio::time::timeout(Duration::from_secs(5), writing)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn dropping_execution_closes_output_and_descendants() {
    let directory = tempfile::tempdir().unwrap();
    let mut running = Running::start(directory.path(), "idle").await;
    running.ready().await;
    let child = child(directory.path());
    let task = running.task.take().unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    exited(&child).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while running.output.changed().await.is_ok() {}
    })
    .await
    .expect("dropped PTY reader did not close");
}

#[tokio::test]
async fn failed_input_setup_closes_without_a_reader() {
    use portable_pty::PtySystem;
    let directory = tempfile::tempdir().unwrap();
    let pair = portable_pty::NativePtySystem::default()
        .inherit_cursor(false)
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", FIXTURE, "--nocapture"]);
    command.env_clear();
    for (key, value) in environment(directory.path()) {
        command.env(key, value);
    }
    command.env("SAILRY_PTY_CASE", "flood");
    command.env("SAILRY_PTY_ROOT", directory.path());
    command.cwd(directory.path());
    let job = Job::new().unwrap();
    job.configure(&mut command);
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    tokio::time::timeout(Duration::from_secs(10), async {
        while !directory.path().join("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("flooding fixture did not start");
    let writer = pair.master.take_writer().unwrap();
    let output: pty::Output = Arc::new(|_| true);
    assert!(pty::Io::start(&*pair.master, output).is_err());
    let (done, finished) = tokio::sync::oneshot::channel();
    let closing = std::thread::spawn(move || {
        drop(job);
        let _ = child.kill();
        drop(writer);
        drop(pair.master);
        child.wait().unwrap();
        done.send(()).unwrap();
    });
    tokio::time::timeout(Duration::from_secs(10), finished)
        .await
        .expect("failed PTY startup did not close")
        .unwrap();
    closing.join().unwrap();
}

#[test]
fn fixture() {
    let Ok(case) = std::env::var("SAILRY_PTY_CASE") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("SAILRY_PTY_ROOT").unwrap());
    match case.as_str() {
        "tail" => {
            let mut output = std::io::stdout().lock();
            for _ in 0..64 {
                output.write_all(&[b'x'; 4096]).unwrap();
                output.flush().unwrap();
                std::thread::sleep(Duration::from_millis(25));
            }
            output
                .write_all("\r\n中文 🙂 final tail\r\n".as_bytes())
                .unwrap();
            output.flush().unwrap();
            std::process::exit(7);
        }
        "reply" => {
            println!("fixture-ready");
            std::io::stdout().flush().unwrap();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            println!("input:{}", input.trim_end());
            std::io::stdout().flush().unwrap();
            std::process::exit(7);
        }
        "idle" | "exit" => {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", FIXTURE, "--nocapture"])
                .env("SAILRY_PTY_CASE", "child")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            std::fs::write(root.join("child.pid"), child.id().to_string()).unwrap();
            println!("fixture-ready");
            std::io::stdout().flush().unwrap();
            let _ = std::io::stdin().read(&mut [0u8; 1]);
            if case == "exit" {
                std::process::exit(7);
            }
            std::thread::sleep(Duration::from_secs(60));
            let _ = child.wait();
        }
        "child" => std::thread::sleep(Duration::from_secs(60)),
        "flood" => {
            std::fs::write(root.join("started"), "ready").unwrap();
            std::io::stdout()
                .write_all(&vec![b'x'; 16 * 1024 * 1024])
                .unwrap();
        }
        _ => panic!("unknown PTY fixture case"),
    }
}
