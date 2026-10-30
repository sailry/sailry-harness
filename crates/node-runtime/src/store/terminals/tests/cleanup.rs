use super::{
    fixture::{Fixture, launch},
    *,
};
use std::{
    path::Path,
    process::{Child, Command as ProcessCommand},
    time::Duration,
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

struct Sibling(Child);

impl Drop for Sibling {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn alive(pid: u32) -> bool {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system
        .process(pid)
        .is_some_and(|process| process.status() != ProcessStatus::Zombie)
}

async fn recorded(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(path)
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("terminal must record its own fixture process")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closes_background_job_groups_without_stopping_siblings() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for remote in [false, true] {
        let fixture = Fixture::new(remote, "/bin/bash").await;
        let sibling = Sibling(ProcessCommand::new("/bin/sleep").arg("60").spawn().unwrap());
        let Output::Terminal(info) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::CreateTerminal(launch(fixture.worktrees[0]))),
            )
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        // Isolated generated paths are shell-quoted; no user path or command is interpolated.
        let leader = fixture.directory.path().join("leader");
        let background = fixture.directory.path().join("background");
        let quoted = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
        let text = format!(
            "set -m; trap '' HUP; echo $$ > {}; /bin/sleep 60 & echo $! > {}; wait",
            quoted(&leader),
            quoted(&background),
        );
        fixture.input(&info, terminal::Input::Paste { text }).await;
        fixture
            .input(
                &info,
                terminal::Input::Key {
                    event: terminal::KeyEvent {
                        key: terminal::Key::Enter,
                        action: terminal::Action::Press,
                        modifiers: terminal::Modifiers::default(),
                        utf8: None,
                        unshifted_codepoint: None,
                    },
                },
            )
            .await;
        let root = recorded(&leader).await;
        let child = recorded(&background).await;
        assert!(alive(root) && alive(child));
        // SAFETY: these calls only read the IDs recorded by this isolated fixture's shell.
        let (session, group) = unsafe { (libc::getsid(child as i32), libc::getpgid(child as i32)) };
        assert_eq!(session, root as i32);
        assert_ne!(
            group, root as i32,
            "job control must create a separate process group"
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::CloseTerminal {
                worktree: fixture.worktrees[0],
                terminal: info.id,
            }))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while alive(child) || alive(root) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("closing the owned PTY must stop its background job groups");
        assert!(
            alive(sibling.0.id()),
            "an unrelated sibling must remain alive"
        );
        fixture.close().await;
    }
}
