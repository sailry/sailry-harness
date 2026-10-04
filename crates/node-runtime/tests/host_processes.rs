use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, Output};
use std::{
    process::{Child, Command as OsCommand, Stdio},
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System};

struct Fixture(Child);
impl Fixture {
    fn start() -> Self {
        Self(
            OsCommand::new(std::env::current_exe().unwrap())
                .args(["fixture_process", "--ignored", "--exact"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn identity(&self) -> (u32, u64) {
        let pid = self.0.id();
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            true,
            ProcessRefreshKind::nothing(),
        );
        let started = system.process(Pid::from_u32(pid)).unwrap().start_time();
        assert!(started > 0);
        (pid, started)
    }
    async fn exited(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.0.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "fixture process did not exit");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "isolated subprocess fixture"]
fn fixture_process() {
    std::thread::sleep(Duration::from_secs(60));
}

#[tokio::test]
async fn controls_isolated_processes() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let local = Client::new(node.local());
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        for force in [false, true] {
            let mut fixture = Fixture::start();
            let (pid, started_at_secs) = fixture.identity();
            if !force && !sysinfo::SUPPORTED_SIGNALS.contains(&Signal::Term) {
                let result = client
                    .execute(client.prepare(Command::StopHostProcess {
                        pid,
                        started_at_secs,
                        force,
                    }))
                    .await;
                assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
                assert!(fixture.0.try_wait().unwrap().is_none());
                continue;
            }
            let stale = client.prepare(Command::StopHostProcess {
                pid,
                started_at_secs: started_at_secs + 1,
                force,
            });
            assert_eq!(
                client.execute(stale).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            assert!(fixture.0.try_wait().unwrap().is_none());
            let request = client.prepare(Command::StopHostProcess {
                pid,
                started_at_secs,
                force,
            });
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            assert_eq!(
                admission.completion.await.unwrap().unwrap(),
                Output::HostProcessSignalled {
                    pid,
                    started_at_secs
                }
            );
            fixture.exited().await;
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::HostProcessSignalled {
                    pid,
                    started_at_secs
                }
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::StopHostProcess {
                        pid,
                        started_at_secs,
                        force
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        for pid in [0, 1, std::process::id()] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::StopHostProcess {
                        pid,
                        started_at_secs: 1,
                        force: true
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert!(matches!(
            client
                .execute(client.prepare(Command::InspectHost))
                .await
                .unwrap(),
            Output::HostInfo(_)
        ));
    }
    node.link()
        .set_trust(
            sailry_protocol::NodeId(*controller.handle().address().id.as_bytes()),
            false,
        )
        .await
        .unwrap();
    assert!(
        remote
            .execute(remote.prepare(Command::StopHostProcess {
                pid: 0,
                started_at_secs: 1,
                force: true
            }))
            .await
            .is_err()
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
