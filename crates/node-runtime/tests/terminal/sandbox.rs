#![cfg(any(target_os = "macos", target_os = "linux"))]
use super::*;
use sailry_protocol::process::Access;

async fn client(node: &Node, controller: &Link, remote: bool) -> Client {
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    Client::new(if remote {
        controller.handle().remote(address)
    } else {
        node.local()
    })
}

#[tokio::test]
async fn preserves_access_when_reopening_shells() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = directory.path().join("node");
        let mut node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let mut current = client(&node, &controller, remote).await;
        let Output::Project(project) = current
            .execute(current.prepare(Command::RegisterProject {
                name: "Sandbox fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = current
            .execute(current.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id))
            .unwrap()
            .id;
        current
            .execute(current.prepare(Command::SaveTerminalSettings(Settings {
                revision: 0,
                shell: "/bin/sh".into(),
                environment: Default::default(),
            })))
            .await
            .unwrap();
        let mut terminals = Vec::new();
        for access in [Access::Workspace, Access::Unrestricted] {
            let Output::Terminal(info) = current
                .execute(current.prepare(Command::CreateTerminal(Launch {
                    access,
                    ..launch(worktree)
                })))
                .await
                .unwrap()
            else {
                panic!("terminal expected")
            };
            assert_eq!(info.access, Some(access));
            terminals.push(info);
        }
        let old_id = node.id();
        node.shutdown().await.unwrap();
        controller.handle().disconnect(old_id).await;
        node = Node::start_empty(&profile).await.unwrap();
        current = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            Client::new(node.local())
        };
        let Output::Terminals(stored) = current
            .execute(current.prepare(Command::ListTerminals { worktree }))
            .await
            .unwrap()
        else {
            panic!("terminals expected")
        };
        assert_eq!(stored.len(), 2);
        for original in &terminals {
            let stored = stored.iter().find(|info| info.id == original.id).unwrap();
            assert_eq!(stored.access, original.access);
            assert_eq!(stored.status, Status::Stopped);
            let launch = launch(worktree);
            let request = current.prepare(Command::OpenTerminal {
                terminal: original.id,
                viewport: launch.viewport,
                appearance: launch.appearance,
            });
            let Output::Terminal(info) = current.execute(request.clone()).await.unwrap() else {
                panic!("terminal expected")
            };
            assert_eq!(info.access, original.access);
            assert_eq!(
                current.execute(request).await.unwrap(),
                Output::Terminal(info.clone())
            );
            let mut stream = current.subscribe_terminal(info.id).await.unwrap();
            let mut projection = Projection::new(node.id(), info.id, 1);
            if original.access == Some(Access::Workspace) {
                command(&current, &info, "printf inside > workspace.txt; if printf outside > ../blocked.txt; then printf 'pty-%s\\n' escaped; else printf 'pty-%s\\n' confined; fi").await;
                until(&mut *stream, &mut projection, "pty-confined").await;
                assert_eq!(
                    std::fs::read_to_string(root.join("workspace.txt")).unwrap(),
                    "inside"
                );
                assert!(!directory.path().join("blocked.txt").exists());
            } else {
                command(
                    &current,
                    &info,
                    "printf allowed > ../allowed.txt; printf 'pty-%s\\n' unrestricted",
                )
                .await;
                until(&mut *stream, &mut projection, "pty-unrestricted").await;
                assert_eq!(
                    std::fs::read_to_string(directory.path().join("allowed.txt")).unwrap(),
                    "allowed"
                );
            }
            current
                .execute(current.prepare(Command::CloseTerminal {
                    worktree,
                    terminal: info.id,
                }))
                .await
                .unwrap();
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn blocks_network_in_workspace_pty() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let current = client(&node, &controller, remote).await;
        let Output::Project(project) = current
            .execute(current.prepare(Command::RegisterProject {
                name: "Network fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = current
            .execute(current.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id))
            .unwrap()
            .id;
        current
            .execute(current.prepare(Command::SaveTerminalSettings(Settings {
                revision: 0,
                shell: "/bin/sh".into(),
                environment: Default::default(),
            })))
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let curl = which::which("curl").expect("sandbox acceptance requires curl");
        let Output::Terminal(info) = current
            .execute(current.prepare(Command::CreateTerminal(launch(worktree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = current.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(&current, &info, &format!("if '{}' --silent --max-time 1 http://{address}/; then printf 'network-%s\\n' allowed; else printf 'network-%s\\n' blocked; fi",curl.to_str().unwrap().replace('\'',"'\\''"))).await;
        until(&mut *stream, &mut projection, "network-blocked").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        current
            .execute(current.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn controls_only_owned_foreground_group() {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = isolated(
            &directory.path().join("node"),
            &directory.path().join("shell-home"),
        )
        .await;
        let private = node.profile().join("private.txt");
        std::fs::write(&private, "protected").unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let current = client(&node, &controller, remote).await;
        let Output::Project(project) = current
            .execute(current.prepare(Command::RegisterProject {
                name: "Job control fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = current
            .execute(current.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id) && tree.main)
            .unwrap()
            .id;
        current
            .execute(current.prepare(Command::SaveTerminalSettings(Settings {
                shell: "/bin/zsh".into(),
                ..Default::default()
            })))
            .await
            .unwrap();
        let sibling = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let device = sibling.master.tty_name().unwrap();
        let Output::Terminal(info) = current
            .execute(current.prepare(Command::CreateTerminal(launch(worktree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(info.access, Some(Access::Workspace));
        let mut stream = current.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(
            &current,
            &info,
            "/usr/bin/stat -L -f '%HT:%r' /dev/fd/0 > tty.txt; printf 'monitor-%s\\n' \"$options[monitor]\"",
        )
        .await;
        until(&mut *stream, &mut projection, "monitor-on").await;
        assert!(
            !content(&projection.snapshot().unwrap().screen).contains("can't set tty pgrp"),
            "{:?}",
            projection.snapshot().unwrap()
        );
        let owned = std::fs::read_to_string(root.join("tty.txt")).unwrap();
        let (kind, owned) = owned.trim().split_once(':').unwrap();
        assert_eq!(kind, "Character Device");
        let owned: u64 = owned.parse().unwrap();
        let metadata = std::fs::metadata(&device).unwrap();
        assert!(metadata.file_type().is_char_device());
        assert_ne!(owned, metadata.rdev());
        command(
            &current,
            &info,
            "sh -c 'printf \"\\033]9;4;3\\007%s%s\\n\" child- started; exec sleep 30'",
        )
        .await;
        until(&mut *stream, &mut projection, "child-started").await;
        assert_eq!(
            projection.snapshot().unwrap().info.activity.unwrap().state,
            Activity::Working
        );
        current
            .execute(current.prepare(Command::InputTerminal {
                terminal: info.id,
                revision: info.revision,
                input: Input::Key {
                    event: KeyEvent {
                        key: Key::Character {
                            codepoint: u32::from('c'),
                        },
                        action: Action::Press,
                        modifiers: Modifiers {
                            control: true,
                            ..Default::default()
                        },
                        utf8: Some("c".into()),
                        unshifted_codepoint: Some(u32::from('c')),
                    },
                },
            }))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(1, stream.next().await.unwrap()).unwrap();
                if projection.snapshot().is_some_and(|snapshot| {
                    snapshot
                        .info
                        .activity
                        .is_some_and(|activity| activity.state == Activity::Idle)
                }) {
                    break;
                }
            }
        })
        .await
        .expect("foreground interruption deadline");
        command(
            &current,
            &info,
            "fixture_status=$?; printf 'foreground-%s:%s\\n' recovered \"$fixture_status\"",
        )
        .await;
        until(&mut *stream, &mut projection, "foreground-recovered:130").await;
        assert_eq!(projection.snapshot().unwrap().info.status, Status::Running);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let quote =
            |path: &std::path::Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
        let private = quote(&private);
        let device = quote(&device);
        command(
            &current,
            &info,
            &format!(
                "printf inside > workspace.txt || exit 90; \
                 if (printf escaped > ../escaped.txt) 2>/dev/null; then exit 91; fi; \
                 if (printf changed > {private}) 2>/dev/null; then exit 92; fi; \
                 if cat {private} > /dev/null 2>/dev/null; then exit 93; fi; \
                 if (exec 7< {device}) 2>/dev/null; then exit 94; fi; \
                 if (printf stolen > {device}) 2>/dev/null; then exit 95; fi; \
                 if /usr/bin/curl --silent --max-time 1 http://{address}/; then exit 96; fi; \
                 printf 'guards-%s\\n' confined"
            ),
        )
        .await;
        until(&mut *stream, &mut projection, "guards-confined").await;
        assert_eq!(
            std::fs::read_to_string(root.join("workspace.txt")).unwrap(),
            "inside"
        );
        assert!(!directory.path().join("escaped.txt").exists());
        assert_eq!(
            std::fs::read_to_string(node.profile().join("private.txt")).unwrap(),
            "protected"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        current
            .execute(current.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        drop(sibling);
    }
}
