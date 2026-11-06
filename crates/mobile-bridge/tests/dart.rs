//! Host-side Dart FFI acceptance. Mobile device execution is a separate check.
use configuration::support as agent_support;

#[path = "../../node-runtime/tests/support/database/native.rs"]
mod database_fixture;

#[path = "dart/activity.rs"]
mod activity;
#[path = "dart/approvals.rs"]
mod approvals;
#[path = "dart/attachments.rs"]
mod attachments;
#[path = "dart/catalog.rs"]
mod catalog;
#[path = "dart/checkpoints.rs"]
mod checkpoints;
#[path = "dart/compaction.rs"]
mod compaction;
#[path = "dart/configuration.rs"]
mod configuration;
#[path = "dart/cost.rs"]
mod cost;
#[path = "dart/databases.rs"]
mod databases;
#[path = "dart/delegation.rs"]
mod delegation;
#[path = "dart/discovery.rs"]
mod discovery;
#[path = "dart/forks.rs"]
mod forks;
#[path = "dart/goals.rs"]
mod goals;
#[path = "dart/login.rs"]
mod login;
#[cfg(unix)]
#[path = "dart/mcp.rs"]
mod mcp;
#[path = "dart/mcp_input.rs"]
mod mcp_input;
#[path = "../../node-runtime/tests/support/mcp.rs"]
#[allow(dead_code)]
mod mcp_peer;
#[path = "dart/media.rs"]
mod media;
#[path = "dart/native.rs"]
mod native;
#[path = "dart/overview.rs"]
mod overview;
#[path = "dart/paging.rs"]
mod paging;
#[cfg(unix)]
#[path = "dart/permissions.rs"]
mod permissions;
#[path = "dart/plan_review.rs"]
mod plan_review;
#[cfg(unix)]
#[path = "dart/planning.rs"]
mod planning;
#[path = "dart/ports.rs"]
mod ports;
#[cfg(unix)]
#[path = "dart/process.rs"]
mod process;
#[path = "dart/progress.rs"]
mod progress;
#[path = "dart/questions.rs"]
mod questions;
#[path = "dart/queue.rs"]
mod queue;
#[path = "dart/references.rs"]
mod references;
#[path = "dart/rewind.rs"]
mod rewind;
#[path = "dart/roles.rs"]
mod roles;
#[path = "dart/search.rs"]
mod search;
#[path = "dart/session_order.rs"]
mod session_order;
#[cfg(unix)]
#[path = "dart/skills.rs"]
mod skills;
#[cfg(unix)]
#[path = "dart/ssh.rs"]
mod ssh;
#[cfg(unix)]
#[path = "dart/terminal_tools.rs"]
mod terminal_tools;
#[path = "dart/tools.rs"]
mod tools;
#[path = "dart/unassigned.rs"]
mod unassigned;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn controls_a_real_node() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let child = command("contract.dart", directory.path(), invitation.ticket())
        .env("SAILRY_PROJECT_PATH", project)
        .spawn()
        .unwrap();
    let status = wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
}

fn command(script: &str, directory: &std::path::Path, ticket: &str) -> std::process::Command {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let library = std::env::var_os("SAILRY_BRIDGE_LIBRARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(if cfg!(target_os = "macos") {
                    "libsailry_mobile_bridge.dylib"
                } else {
                    "libsailry_mobile_bridge.so"
                })
        });
    let mut command = std::process::Command::new("dart");
    command
        .args(["run", &format!("bin/{script}")])
        .current_dir(root.join("tests/mobile-contract"))
        .env("SAILRY_BRIDGE_LIBRARY", library.canonicalize().unwrap())
        .env("SAILRY_CONTROLLER_PROFILE", directory.join("controller"))
        .env("SAILRY_INVITATION", ticket)
        .env(
            "SAILRY_READ_TOOL",
            agent_support::plugin_tool("files", "read_file"),
        )
        .env(
            "SAILRY_WRITE_TOOL",
            agent_support::plugin_tool("files", "write_file"),
        )
        .env(
            "SAILRY_RUN_TOOL",
            agent_support::plugin_tool("commands", "run_command"),
        )
        .env("SAILRY_SPAWN_TOOL", "spawn_agent")
        .env("SAILRY_PROJECT_PATH", directory)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

/// Recent script output and the time of its last line.
struct Transcript {
    progress: std::time::Instant,
    tail: std::collections::VecDeque<String>,
}

/// Echoes a script stream line by line while recording progress and its tail.
fn forward(
    stream: impl std::io::Read + Send + 'static,
    error: bool,
    transcript: std::sync::Arc<std::sync::Mutex<Transcript>>,
) -> std::thread::JoinHandle<()> {
    use std::io::BufRead;
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stream).split(b'\n') {
            let Ok(line) = line else {
                break;
            };
            let line = String::from_utf8_lossy(&line).into_owned();
            if error {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
            let mut transcript = transcript.lock().unwrap();
            transcript.progress = std::time::Instant::now();
            if transcript.tail.len() == 40 {
                transcript.tail.pop_front();
            }
            transcript.tail.push_back(line);
        }
    })
}

/// Waits for a Dart contract script. Each output line restarts the progress
/// window, the ceiling bounds the whole script, and a timeout reports the tail.
fn wait(mut child: std::process::Child) -> std::process::ExitStatus {
    use std::time::{Duration, Instant};
    const WINDOW: Duration = Duration::from_secs(30);
    const CEILING: Duration = Duration::from_secs(240);
    let started = Instant::now();
    let transcript = std::sync::Arc::new(std::sync::Mutex::new(Transcript {
        progress: started,
        tail: Default::default(),
    }));
    let readers = [
        child
            .stdout
            .take()
            .map(|stream| forward(stream, false, transcript.clone())),
        child
            .stderr
            .take()
            .map(|stream| forward(stream, true, transcript.clone())),
    ];
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            // A lingering grandchild may hold the pipes open, so draining is bounded.
            let drained = Instant::now() + Duration::from_secs(1);
            while readers.iter().flatten().any(|reader| !reader.is_finished())
                && Instant::now() < drained
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            return status;
        }
        let now = Instant::now();
        let progress = transcript.lock().unwrap().progress;
        let bound = if now >= started + CEILING {
            "overall ceiling"
        } else if now >= progress + WINDOW {
            "progress window"
        } else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        let _ = child.kill();
        let _ = child.wait();
        let tail = Vec::from(transcript.lock().unwrap().tail.clone()).join("\n");
        panic!(
            "Dart contract hit its {bound} (window={WINDOW:?}, ceiling={CEILING:?}); last output:\n{tail}"
        );
    }
}
