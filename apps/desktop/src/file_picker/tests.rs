use super::*;
use core::prelude::v1::test;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

mod selection {
    use super::*;
    fn listing(separator: char) -> Listing {
        Listing {
            separator,
            locations: Vec::new(),
            parent: None,
            directory: sailry_protocol::Directory {
                path: if separator == '/' {
                    "/node/目录"
                } else {
                    "D:\\node\\目录"
                }
                .into(),
                entries: vec![
                    FileEntry {
                        name: "folder".into(),
                        kind: EntryKind::Directory,
                        size: 0,
                    },
                    FileEntry {
                        name: "Alpha.md".into(),
                        kind: EntryKind::File,
                        size: 1,
                    },
                    FileEntry {
                        name: ".hidden".into(),
                        kind: EntryKind::File,
                        size: 1,
                    },
                    FileEntry {
                        name: "second.txt".into(),
                        kind: EntryKind::File,
                        size: 1,
                    },
                ],
                truncated: false,
                unsupported_names: 0,
                revision: "fixture".into(),
                next: None,
            },
        }
    }
    #[test]
    fn filters_keep_source_indices() {
        let listing = listing('/');
        assert_eq!(
            super::super::selection::visible(&listing.directory.entries, false, "ALPHA"),
            vec![1]
        );
        assert_eq!(
            super::super::selection::visible(&listing.directory.entries, false, ""),
            vec![0, 1, 3]
        );
        assert_eq!(
            super::super::selection::visible(&listing.directory.entries, true, "hidden"),
            vec![2]
        );
    }
    #[test]
    fn directory_uses_folder_or_current() {
        let listing = listing('/');
        assert_eq!(
            super::super::selection::resolve(&Mode::Directory, &listing, &BTreeSet::from([1]), ""),
            Some(Selection::Directory("/node/目录".into()))
        );
        assert_eq!(
            super::super::selection::resolve(
                &Mode::Directory,
                &listing,
                &BTreeSet::from([0, 1]),
                ""
            ),
            Some(Selection::Directory("/node/目录/folder".into()))
        );
    }
    #[test]
    fn files_require_regular_selection() {
        let listing = listing('/');
        assert_eq!(
            super::super::selection::resolve(
                &Mode::Files { multiple: false },
                &listing,
                &BTreeSet::from([0]),
                ""
            ),
            None
        );
        assert_eq!(
            super::super::selection::resolve(
                &Mode::Files { multiple: false },
                &listing,
                &BTreeSet::from([1, 3]),
                ""
            ),
            None
        );
        assert_eq!(
            super::super::selection::resolve(
                &Mode::Files { multiple: true },
                &listing,
                &BTreeSet::from([0, 1, 3]),
                ""
            ),
            Some(Selection::Files(vec![
                "/node/目录/Alpha.md".into(),
                "/node/目录/second.txt".into()
            ]))
        );
    }
    #[test]
    fn save_uses_node_separator() {
        let mode = Mode::Save {
            suggested_name: String::new(),
        };
        for separator in ['/', '\\'] {
            let listing = listing(separator);
            assert_eq!(
                super::super::selection::resolve(&mode, &listing, &BTreeSet::new(), "结果.md"),
                Some(Selection::Save(format!(
                    "{}{separator}结果.md",
                    listing.directory.path
                )))
            );
            for name in ["", ".", "..", "parent/file", "parent\\file", "bad\0name"] {
                assert_eq!(
                    super::super::selection::resolve(&mode, &listing, &BTreeSet::new(), name),
                    None
                );
            }
        }
    }
}

fn wait(cx: &mut VisualTestContext, picker: &Entity<Picker>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            !picker.read(cx).busy
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "picker update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn modes_keep_host_paths(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::new(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let path = fixture.directory.path().join("chooser");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("folder")).unwrap();
        std::fs::write(path.join("Alpha.md"), "alpha").unwrap();
        std::fs::write(path.join("second.txt"), "second").unwrap();
        std::fs::write(path.join(".hidden"), "hidden").unwrap();
        let path = path.canonicalize().unwrap();
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
            Root::new(shell, window, cx)
        });
        for mode in [
            Mode::Directory,
            Mode::Files { multiple: false },
            Mode::Files { multiple: true },
            Mode::Save {
                suggested_name: "saved.md".into(),
            },
        ] {
            let selections = Arc::new(Mutex::new(Vec::new()));
            let capture = selections.clone();
            let mut reject_name = matches!(mode, Mode::Save { .. });
            let picker = visual.update(|window, cx| {
                open(
                    Options {
                        client: Some(Arc::new(Client::new(fixture.transport.clone()))),
                        preview_host: 0,
                        initial: Some(path.to_str().unwrap().into()),
                        mode: mode.clone(),
                        title: "directory_title",
                        confirm: "directory_open",
                    },
                    move |selection, _, _| {
                        if reject_name {
                            reject_name = false;
                            return Err("directory_action_failed");
                        }
                        capture.lock().unwrap().push(selection);
                        Ok(Completion::Close)
                    },
                    window,
                    cx,
                )
            });
            wait(visual, &picker);
            visual.update(|window, cx| {
                picker.update(cx, |picker, cx| {
                    assert!(!picker.grid);
                    assert_eq!(picker.visible.len(), 3);
                    let first = picker
                        .entries()
                        .iter()
                        .position(|entry| entry.name == "Alpha.md")
                        .unwrap();
                    let second = picker
                        .entries()
                        .iter()
                        .position(|entry| entry.name == "second.txt")
                        .unwrap();
                    picker.choose(first, false, window, cx);
                    assert_eq!(
                        picker.table.read(cx).delegate().multiple,
                        matches!(mode, Mode::Files { multiple: true })
                    );
                    if matches!(mode, Mode::Files { multiple: true }) {
                        picker.choose(second, true, window, cx);
                        assert_eq!(
                            picker.table.read(cx).delegate().selected,
                            BTreeSet::from([first, second])
                        );
                        picker.choose(second, true, window, cx);
                        assert_eq!(
                            picker.table.read(cx).delegate().selected,
                            BTreeSet::from([first])
                        );
                        picker.choose(second, true, window, cx);
                    }
                    if matches!(mode, Mode::Save { .. }) {
                        picker
                            .filename
                            .update(cx, |input, cx| input.set_value("saved.md", window, cx));
                    }
                    picker.confirm(window, cx);
                })
            });
            if matches!(mode, Mode::Save { .. }) {
                visual.update(|window, cx| {
                    picker.update(cx, |picker, cx| {
                        assert_eq!(picker.error, Some("directory_action_failed"));
                        picker
                            .filename
                            .update(cx, |input, cx| input.focus(window, cx));
                    })
                });
                visual.simulate_keystrokes("secondary-a");
                visual.simulate_input("retry.md");
                visual.simulate_keystrokes("secondary-a");
                visual.simulate_input("saved.md");
                visual.run_until_parked();
                visual.update(|_, cx| {
                    assert!(picker.read(cx).error.is_none());
                    assert_eq!(picker.read(cx).filename.read(cx).value(), "saved.md");
                });
                visual.simulate_keystrokes("enter");
                visual.run_until_parked();
            }
            let expected = match mode {
                Mode::Directory => Selection::Directory(path.to_str().unwrap().into()),
                Mode::Files { multiple } => Selection::Files(if multiple {
                    vec![
                        path.join("Alpha.md").to_str().unwrap().into(),
                        path.join("second.txt").to_str().unwrap().into(),
                    ]
                } else {
                    vec![path.join("Alpha.md").to_str().unwrap().into()]
                }),
                Mode::Save { .. } => {
                    Selection::Save(path.join("saved.md").to_str().unwrap().into())
                }
            };
            assert_eq!(*selections.lock().unwrap(), vec![expected]);
            assert!(!path.join("saved.md").exists());
        }
        let selections = Arc::new(Mutex::new(Vec::new()));
        let capture = selections.clone();
        let picker = visual.update(|window, cx| {
            open(
                Options {
                    client: Some(Arc::new(Client::new(fixture.transport.clone()))),
                    preview_host: 0,
                    initial: Some(path.to_str().unwrap().into()),
                    mode: Mode::Files { multiple: true },
                    title: "directory_title",
                    confirm: "directory_open",
                },
                move |selection, _, _| {
                    capture.lock().unwrap().push(selection);
                    Ok(Completion::Close)
                },
                window,
                cx,
            )
        });
        wait(visual, &picker);
        visual.update(|window, cx| {
            picker.update(cx, |picker, cx| {
                picker
                    .filter
                    .update(cx, |input, cx| input.set_value("SECOND", window, cx));
                picker.rebuild(cx);
                assert_eq!(picker.visible.len(), 1);
                let index = picker.visible[0];
                assert_eq!(picker.entries()[index].name, "second.txt");
                picker.choose(index, false, window, cx);
                picker.grid = true;
                assert_eq!(
                    picker.selection(cx),
                    Some(Selection::Files(vec![
                        path.join("second.txt").to_str().unwrap().into()
                    ]))
                );
                picker.copy(false);
                assert_eq!(
                    picker.clipboard.as_ref().unwrap().0,
                    vec![path.join("second.txt").to_str().unwrap().to_owned()]
                );
                picker.refresh(window, cx);
            })
        });
        wait(visual, &picker);
        visual.update(|window, cx| {
            picker.update(cx, |picker, cx| {
                assert_eq!(picker.visible.len(), 1);
                assert_eq!(picker.selected.len(), 1);
                picker.navigate(
                    Some(path.join("folder").to_str().unwrap().into()),
                    Visit::Push,
                    window,
                    cx,
                );
            })
        });
        wait(visual, &picker);
        visual.update(|_, cx| {
            assert!(picker.read(cx).filter.read(cx).value().is_empty());
        });
        visual.update(|window, cx| picker.update(cx, |picker, cx| picker.close(window, cx)));
        assert!(selections.lock().unwrap().is_empty());
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !matches!(request.command, Command::ManageFiles(_)))
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
