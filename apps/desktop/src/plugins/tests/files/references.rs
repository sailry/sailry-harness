use super::*;
use crate::plugins::file_transfers::system::{self, Action};
use std::path::{Path, PathBuf};

#[gpui::test]
fn ordinary_deliverables_open_in_the_editor(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let cases = [
            ("report.txt", "text/plain", "8 checks passed\n"),
            ("README", "application/octet-stream", "Fixture handoff\n"),
            ("data.json", "application/json", "{\"count\":8}\n"),
            ("stats.py", "text/plain", "print(8)\n"),
            ("verify.cjs", "application/octet-stream", "console.log(8)\n"),
            ("execution.log", "text/plain", "8 checks passed\n"),
            (
                "HANDOFF.md",
                "text/markdown",
                "# Report\n\n**8 checks passed**\n",
            ),
        ];
        for (path, _, text) in cases {
            std::fs::write(fixture.directory.path().join("project").join(path), text).unwrap();
        }
        let (shell, visual) = mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).connected() && chat.read(cx).preview_available("text/html", cx)
            })
        });
        for (path, mime, text) in cases {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let chat = shell.current_chat().unwrap().clone();
                    shell.open_artifact(
                        chat,
                        fixture.session.worktree,
                        sailry_protocol::tool::File {
                            path: path.into(),
                            name: None,
                            mime: mime.into(),
                            size: None,
                            revision: None,
                        },
                        window,
                        cx,
                    );
                })
            });
            let panel = shell.read_with(visual, |shell, _| match &shell.side_resource {
                Some(SideResource::Plugin(panel)) => panel.clone(),
                _ => panic!("file editor expected, not an empty preview"),
            });
            ready(&panel, visual);
            let id = document(&panel, visual, path);
            let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
            let editor =
                controller.read_with(visual, |controller, _| controller.editor(path).unwrap());
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.value().to_string()),
                text
            );
            assert_eq!(
                panel.read_with(visual, |panel, cx| panel.document_scope(cx)),
                Some((fixture.node.id(), fixture.session.worktree))
            );
            if path.ends_with(".md") {
                assert_eq!(
                    visual.update(|_, cx| value(&panel, &id, cx)["mode"].clone()),
                    "document"
                );
                assert_eq!(
                    visual.update(|_, cx| value(&panel, &id, cx)["markdown"].clone()),
                    true
                );
            }
        }
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn unsupported_primary_keeps_explicit_reveal(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let cases: [(&str, &str, &[u8]); 3] = [
            (
                "bundle.zip",
                "application/zip",
                b"PK\x03\x04\0captured archive\xff",
            ),
            (
                "test.db",
                "application/octet-stream",
                b"SQLite format 3\0\xffbinary",
            ),
            ("clip.mp4", "video/mp4", b"\0\0\0\x18ftypcaptured video\xff"),
        ];
        for (path, _, bytes) in cases {
            std::fs::write(fixture.directory.path().join("project").join(path), bytes).unwrap();
        }
        let (shell, visual) = mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            });
            system::take(cx);
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).connected() && chat.read(cx).preview_available("text/html", cx)
            })
        });
        let mut copies = Vec::new();
        for (path, mime, bytes) in cases {
            let failures = visual.update(|window, cx| {
                crate::feedback::tests::count(window, crate::tr("files_open_failed").as_ref(), cx)
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let chat = shell.current_chat().unwrap().clone();
                    shell.open_artifact(
                        chat,
                        fixture.session.worktree,
                        sailry_protocol::tool::File {
                            path: path.into(),
                            name: None,
                            mime: mime.into(),
                            size: None,
                            revision: None,
                        },
                        window,
                        cx,
                    );
                })
            });
            if path == "clip.mp4" {
                let (action, opened) = native_action(visual);
                assert_eq!(action, Action::Open);
                assert_target(&fixture, remote, path, bytes, &opened);
            } else {
                let deadline = Instant::now() + Duration::from_secs(10);
                while visual.update(|window, cx| {
                    crate::feedback::tests::count(
                        window,
                        crate::tr("files_open_failed").as_ref(),
                        cx,
                    )
                }) == failures
                {
                    wait(visual, |_| true);
                    assert!(Instant::now() < deadline, "artifact failure toast deadline");
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert_eq!(
                    visual.update(crate::feedback::tests::summary),
                    crate::tr("files_open_failed")
                );
                assert!(visual.update(|_, cx| system::take(cx).is_empty()));
            }
            // Primary failure does not remove the explicitly requested Reveal.
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.reveal_file_in_system(
                        (fixture.node.id(), fixture.session.worktree),
                        path.into(),
                        window,
                        cx,
                    );
                })
            });
            let (action, opened) = native_action(visual);
            assert_eq!(action, Action::Reveal);
            assert_target(&fixture, remote, path, bytes, &opened);
            assert!(!shell.read_with(visual, |shell, _| {
                matches!(&shell.side_resource, Some(SideResource::Artifact(_)))
            }));
            copies.push((opened, bytes));
        }
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| {
                    request.target == fixture.node.id()
                        && matches!(&request.command, Command::ReadFile { worktree, path }
                    if *worktree == fixture.session.worktree && path == "bundle.zip")
                        && request.plugin.as_ref().is_some_and(|context| {
                            context.worktree == Some(fixture.session.worktree)
                        })
                })
        );
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| {
                    matches!(&request.command, Command::ReadFile { path, .. } if path == "clip.mp4")
                })
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_historical_document(
                    (fixture.node.id(), fixture.session.worktree),
                    "clip.mp4".into(),
                    None,
                    window,
                    cx,
                );
            })
        });
        let (action, opened) = native_action(visual);
        assert_eq!(
            action,
            Action::Open,
            "ordinary source navigation keeps system Open"
        );
        assert_target(&fixture, remote, cases[2].0, cases[2].2, &opened);
        visual.update(|window, _| window.remove_window());
        for (opened, bytes) in copies {
            assert_eq!(
                std::fs::read(opened).unwrap(),
                bytes,
                "revealed copies remain available"
            );
        }
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn missing_editor_reports_open_failure(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        enable(&fixture, false);
        let cases: [(&str, &str, &[u8]); 3] = [
            ("README", "application/octet-stream", b"Captured handoff\n"),
            ("report.txt", "text/plain", b"Captured report\n"),
            (
                "bundle.zip",
                "application/zip",
                b"PK\x03\x04\0captured archive\xff",
            ),
        ];
        for (path, _, bytes) in cases {
            std::fs::write(fixture.directory.path().join("project").join(path), bytes).unwrap();
        }
        image::DynamicImage::new_rgb8(2, 2)
            .save(fixture.directory.path().join("project/picture.png"))
            .unwrap();
        let (shell, visual) = mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            });
            system::take(cx);
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).connected()
                    && chat.read(cx).renderer_unavailable(
                        sailry_protocol::plugin::desktop::ResourceKind::Documents,
                        cx,
                    )
            })
        });
        for (path, mime, _) in cases {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let chat = shell.current_chat().unwrap().clone();
                    shell.open_artifact(
                        chat,
                        fixture.session.worktree,
                        sailry_protocol::tool::File {
                            path: path.into(),
                            name: None,
                            mime: mime.into(),
                            size: None,
                            revision: None,
                        },
                        window,
                        cx,
                    );
                })
            });
            assert_eq!(
                visual.update(crate::feedback::tests::summary),
                crate::tr("files_open_failed")
            );
            assert!(visual.update(|_, cx| system::take(cx).is_empty()));
            assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
        }
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| {
                    matches!(&request.command, Command::ReadFile { path, .. }
                if cases.iter().any(|(target, _, _)| path.as_str() == *target))
                })
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let chat = shell.current_chat().unwrap().clone();
                shell.open_artifact(
                    chat,
                    fixture.session.worktree,
                    sailry_protocol::tool::File {
                        path: "picture.png".into(),
                        name: None,
                        mime: "image/png".into(),
                        size: None,
                        revision: None,
                    },
                    window,
                    cx,
                );
            })
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds("image-lightbox-image").is_none() {
            wait(visual, |_| true);
            assert!(Instant::now() < deadline, "artifact image preview deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(visual.debug_bounds("attachment-image-preview").is_some());
        assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
        assert!(visual.update(|_, cx| system::take(cx).is_empty()));
        visual.update(|window, cx| window.close_dialog(cx));
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}

fn native_action(visual: &mut VisualTestContext) -> (Action, PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let mut actions = visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            system::take(cx)
        });
        if !actions.is_empty() {
            assert_eq!(actions.len(), 1, "one native action expected");
            return actions.remove(0);
        }
        assert!(Instant::now() < deadline, "native file action deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_target(fixture: &Fixture, remote: bool, path: &str, bytes: &[u8], opened: &Path) {
    let source = fixture
        .directory
        .path()
        .join("project")
        .join(path)
        .canonicalize()
        .unwrap();
    assert_eq!(std::fs::read(opened).unwrap(), bytes);
    assert_eq!(
        std::fs::read(&source).unwrap(),
        bytes,
        "the source stays intact"
    );
    assert_eq!(opened.file_name(), Path::new(path).file_name());
    if remote {
        assert_ne!(fixture.node.id(), fixture.controller.id());
        assert_ne!(opened, source, "remote files use a retained local copy");
        assert!(!opened.starts_with(fixture.directory.path().join("project")));
    } else {
        assert_eq!(
            opened, source,
            "local files retain the captured worktree path"
        );
    }
}

#[gpui::test]
fn opens_captured_lines_without_replacing_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let root = fixture.directory.path().join("project");
        std::fs::write(root.join("build notes.md"), "first\nsecond\nthird\n").unwrap();
        let root = root.canonicalize().unwrap();
        let (shell, visual) = mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx);
                shell.open_conversation_link("build%20notes.md#L3".into(), window, cx);
            })
        });
        let panel = shell.read_with(visual, |shell, _| match &shell.side_resource {
            Some(SideResource::Plugin(panel)) => panel.clone(),
            _ => panic!("ordinary document renderer expected"),
        });
        ready(&panel, visual);
        let id = document(&panel, visual, "build notes.md");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let editor = controller.read_with(visual, |controller, _| {
            controller.editor("build notes.md").unwrap()
        });
        wait(visual, |cx| editor.read(cx).cursor_position().line == 2);
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("draft first\ndraft second\ndraft third\n", window, cx)
            });
        });
        wait(visual, |cx| value(&panel, &id, cx)["dirty"] == true);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_conversation_link(
                    format!("{}/build notes.md:2", root.display()).into(),
                    window,
                    cx,
                );
            })
        });
        wait(visual, |cx| editor.read(cx).cursor_position().line == 1);
        assert_eq!(document(&panel, visual, "build notes.md"), id);
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.editor("build notes.md")),
            Some(editor.clone())
        );
        assert_eq!(
            visual.update(|_, cx| documents(&panel, cx)["documents"].as_array().unwrap().len()),
            1
        );
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.value().to_string()),
            "draft first\ndraft second\ndraft third\n"
        );
        assert_eq!(
            visual.update(|_, cx| value(&panel, &id, cx)["dirty"].clone()),
            true
        );
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.session),
            Some(fixture.session.id)
        );
        if remote {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.open_conversation_link("file:///outside/image.png".into(), window, cx);
                });
                assert_eq!(
                    crate::feedback::tests::summary(window, cx).as_ref(),
                    crate::tr("files_remote_path").as_ref()
                );
            });
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(editor);
        drop(shell);
        fixture.close();
    }
}
