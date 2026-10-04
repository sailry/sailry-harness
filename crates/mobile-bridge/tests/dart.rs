//! Host-side Dart FFI acceptance. Mobile device execution is a separate check.
use configuration::support as agent_support;

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
#[path = "dart/web_search.rs"]
mod web_search;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn dart_controls_a_real_node() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let child = command("contract.dart", directory.path(), invitation.ticket())
        .spawn()
        .unwrap();
    let status = wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
}

fn command(script: &str, directory: &std::path::Path, ticket: &str) -> std::process::Command {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let library = root
        .join("target/debug")
        .join(if cfg!(target_os = "macos") {
            "libsailry_mobile_bridge.dylib"
        } else {
            "libsailry_mobile_bridge.so"
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
        .env(
            "SAILRY_SPAWN_TOOL",
            agent_support::plugin_tool("delegation", "spawn_agent"),
        )
        .env("SAILRY_PROJECT_PATH", directory);
    command
}

fn wait(mut child: std::process::Child) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Dart contract deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
