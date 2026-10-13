use super::*;
use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
use sailry_link::{Admission, Pending, Stream, Subscription as LinkSubscription, Transport};
use sailry_protocol::{Fault, NodeId, StreamId, Topic};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
}

fn show(
    cx: &mut TestAppContext,
    client: Arc<Client>,
    path: String,
    mode: Mode,
    grid: bool,
) -> (Entity<Picker>, Arc<AtomicUsize>, &mut VisualTestContext) {
    cx.update(|cx| {
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.file_picker = Some(crate::preferences::FilePicker {
            grid,
            hidden: false,
        });
        cx.set_global(preferences);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
        Root::new(shell, window, cx)
    });
    let confirmations = Arc::new(AtomicUsize::new(0));
    let completed = confirmations.clone();
    let picker = visual.update(|window, cx| {
        open(
            Options {
                client: Some(client),
                preview_host: 0,
                initial: Some(path),
                mode,
                title: "directory_title",
                confirm: "directory_open",
            },
            move |_, _, _| {
                completed.fetch_add(1, Ordering::SeqCst);
                Ok(Completion::KeepOpen)
            },
            window,
            cx,
        )
    });
    (picker, confirmations, visual)
}

#[track_caller]
fn settle(visual: &mut VisualTestContext, predicate: impl Fn(&mut Window, &mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            predicate(window, cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "picker interaction deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn index(picker: &Entity<Picker>, name: &str, cx: &App) -> usize {
    picker
        .read(cx)
        .entries()
        .iter()
        .position(|entry| entry.name == name)
        .unwrap()
}

fn click_row(
    visual: &mut VisualTestContext,
    picker: &Entity<Picker>,
    index: usize,
    modifiers: Modifiers,
) {
    visual.update(|window, cx| {
        window.render_frame(cx);
        let id: ElementId = if picker.read(cx).grid {
            ("directory-entry", index).into()
        } else {
            ("directory-table-row", index).into()
        };
        window
            .within("directory-picker")
            .click_with_modifiers(id, modifiers, cx);
    });
    visual.run_until_parked();
}

fn dismiss(visual: &mut VisualTestContext, picker: &Entity<Picker>) {
    visual.update(|window, cx| picker.update(cx, |picker, cx| picker.close(window, cx)));
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn single_modes_ignore_selection_modifiers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::directory(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let path = fixture.directory.path().join("chooser");
        std::fs::create_dir(&path).unwrap();
        for name in ["first", "second"] {
            std::fs::create_dir(path.join(name)).unwrap();
        }
        let long_name = "Long-目录-資料-with-a-complete-folder-name";
        std::fs::create_dir(path.join(long_name)).unwrap();
        std::fs::write(path.join("Alpha.md"), "alpha").unwrap();
        std::fs::write(path.join("second.txt"), "second").unwrap();
        for mode in [Mode::Directory, Mode::Files { multiple: false }] {
            for grid in [false, true] {
                let (picker, confirmations, visual) = show(
                    cx,
                    Arc::new(Client::new(fixture.transport.clone())),
                    path.to_str().unwrap().into(),
                    mode.clone(),
                    grid,
                );
                wait(visual, &picker);
                if grid {
                    let long = visual.update(|_, cx| index(&picker, long_name, cx));
                    let row = visual.update(|window, cx| {
                        window.render_frame(cx);
                        assert_eq!(
                            window
                                .within("directory-picker")
                                .find(("directory-entry", long))
                                .label(),
                            Some(long_name)
                        );
                        window
                            .within("directory-picker")
                            .find(("directory-entry", long))
                            .bounds()
                    });
                    let selector = match long {
                        0 => "directory-label-0",
                        1 => "directory-label-1",
                        2 => "directory-label-2",
                        _ => panic!("long folder must precede regular files"),
                    };
                    let icon = visual.debug_bounds("file-type-folder").unwrap();
                    let label = visual.debug_bounds(selector).unwrap();
                    let rem = visual.update(|window, _| window.rem_size());
                    assert_eq!(icon.size, size(rem * 2., rem * 2.));
                    assert!(label.left() >= row.left() && label.right() <= row.right());
                    assert!(label.top() >= row.top() + rem * 2.);
                    assert!(label.size.height <= rem * 1.875 + px(1.));
                }
                let location = visual.update(|window, cx| {
                    window.render_frame(cx);
                    window
                        .within("directory-locations-scroll")
                        .find(("directory-location", 0usize))
                        .bounds()
                });
                let label = visual.debug_bounds("directory-location-label-0").unwrap();
                let rem = visual.update(|window, _| window.rem_size());
                assert!(label.left() - location.left() <= rem * 2.5);
                let first = visual.update(|_, cx| index(&picker, "first", cx));
                let second = visual.update(|_, cx| index(&picker, "second", cx));
                click_row(visual, &picker, first, Modifiers::default());
                for modifiers in [
                    Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                    Modifiers {
                        platform: true,
                        ..Default::default()
                    },
                ] {
                    click_row(visual, &picker, second, modifiers);
                    visual.update(|_, cx| {
                        assert_eq!(picker.read(cx).selected, BTreeSet::from([second]));
                    });
                }
                visual.simulate_keystrokes("secondary-a");
                visual.simulate_keystrokes("ctrl-a");
                visual.update(|_, cx| {
                    assert_eq!(picker.read(cx).selected, BTreeSet::from([second]));
                });
                visual.simulate_keystrokes("shift-down");
                visual.update(|_, cx| assert_eq!(picker.read(cx).selected.len(), 1));
                let file = visual.update(|_, cx| index(&picker, "Alpha.md", cx));
                click_row(visual, &picker, file, Modifiers::default());
                visual.update(|window, cx| {
                    window.render_frame(cx);
                    assert_eq!(
                        picker.read(cx).selection(cx).is_none(),
                        matches!(mode, Mode::Directory)
                    );
                    window.click("directory-confirm", cx);
                    assert_eq!(
                        confirmations.load(Ordering::SeqCst),
                        usize::from(!matches!(mode, Mode::Directory))
                    );
                    if matches!(mode, Mode::Directory) {
                        assert!(
                            picker.read(cx).error.is_none(),
                            "disabled confirmation must not validate a selected file"
                        );
                    }
                });
                if matches!(mode, Mode::Directory) {
                    click_row(visual, &picker, first, Modifiers::default());
                    visual.update(|window, cx| {
                        window.render_frame(cx);
                        assert_eq!(
                            picker.read(cx).selection(cx),
                            Some(Selection::Directory(
                                path.join("first")
                                    .canonicalize()
                                    .unwrap()
                                    .to_str()
                                    .unwrap()
                                    .into()
                            ))
                        );
                        window.click("directory-confirm", cx);
                        assert_eq!(confirmations.load(Ordering::SeqCst), 1);
                    });
                }
                dismiss(visual, &picker);
            }
        }
        fixture.close();
    }
}

struct Paused {
    inner: Arc<dyn Transport>,
    browses: std::sync::Mutex<Vec<Command>>,
    hold: AtomicBool,
    entered: AtomicUsize,
    release: tokio::sync::Semaphore,
}

impl Paused {
    fn new(inner: Arc<dyn Transport>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            browses: std::sync::Mutex::new(vec![]),
            hold: AtomicBool::new(true),
            entered: AtomicUsize::new(0),
            release: tokio::sync::Semaphore::new(0),
        })
    }
}

impl Transport for Paused {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn open(&self, id: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        self.inner.open(id)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn LinkSubscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::BrowseFiles { .. }) {
                self.browses.lock().unwrap().push(request.command.clone());
            }
            if matches!(request.command, Command::BrowseFiles { .. })
                && self.hold.swap(false, Ordering::SeqCst)
            {
                self.entered.fetch_add(1, Ordering::SeqCst);
                self.release.acquire().await.unwrap().forget();
            }
            self.inner.dispatch(request).await
        })
    }
}

#[gpui::test]
fn loading_keeps_navigation_and_footer_current(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::directory(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let path = fixture.directory.path().join("chooser");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("child")).unwrap();
        let paused = Paused::new(fixture.transport.clone());
        let (picker, confirmations, visual) = show(
            cx,
            Arc::new(Client::new(paused.clone())),
            path.to_str().unwrap().into(),
            Mode::Directory,
            true,
        );
        settle(visual, |_, _| paused.entered.load(Ordering::SeqCst) == 1);
        assert!(
            visual.debug_bounds("directory-empty").is_none(),
            "a pending first listing is not an empty directory"
        );
        visual.update(|window, cx| {
            assert!(picker.read(cx).busy);
            assert!(picker.read(cx).listing.is_none());
            window.click("directory-confirm", cx);
            assert_eq!(confirmations.load(Ordering::SeqCst), 0);
            assert!(picker.read(cx).error.is_none());
        });
        paused.release.add_permits(1);
        wait(visual, &picker);
        let previous = visual.update(|_, cx| picker.read(cx).listing.clone().unwrap());
        paused.hold.store(true, Ordering::SeqCst);
        visual.update(|window, cx| window.click("directory-up", cx));
        settle(visual, |_, _| paused.entered.load(Ordering::SeqCst) == 2);
        visual.update(|window, cx| {
            assert!(picker.read(cx).busy);
            assert_eq!(picker.read(cx).listing.as_ref(), Some(&previous));
            window.click("directory-up", cx);
            window.click("directory-confirm", cx);
            assert_eq!(confirmations.load(Ordering::SeqCst), 0);
            assert_eq!(paused.entered.load(Ordering::SeqCst), 2);
        });
        paused.release.add_permits(1);
        wait(visual, &picker);
        visual.update(|window, cx| {
            assert_ne!(
                picker.read(cx).listing.as_ref().unwrap().directory.path,
                previous.directory.path
            );
            window.click("directory-confirm", cx);
            assert_eq!(confirmations.load(Ordering::SeqCst), 1);
            assert!(picker.read(cx).error.is_none());
        });
        dismiss(visual, &picker);
        fixture.close();
    }
}

fn crumb(visual: &mut VisualTestContext, name: &str) -> usize {
    visual.update(|window, cx| {
        window.render_frame(cx);
        (0..64)
            .find(|index| {
                window
                    .within("directory-address")
                    .try_find(("directory-crumb", *index))
                    .is_some_and(|item| item.label() == Some(name))
            })
            .unwrap()
    })
}

#[gpui::test]
fn breadcrumbs_list_children_without_changing_history(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::directory(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let path = fixture.directory.path().join("chooser");
        std::fs::create_dir(&path).unwrap();
        for name in ["Alpha", "Beta", ".hidden"] {
            std::fs::create_dir(path.join(name)).unwrap();
        }
        std::fs::write(path.join("leaf.txt"), "leaf").unwrap();
        for index in 0..sailry_protocol::MAX_DIRECTORY_ENTRIES {
            std::fs::create_dir(path.join(format!("child-{index:03}"))).unwrap();
        }
        let observed = Paused::new(fixture.transport.clone());
        observed.hold.store(false, Ordering::SeqCst);
        let (picker, _, visual) = show(
            cx,
            Arc::new(Client::new(observed.clone())),
            path.join("Alpha").to_str().unwrap().into(),
            Mode::Directory,
            false,
        );
        wait(visual, &picker);
        let history = visual.update(|_, cx| picker.read(cx).history.clone());
        let parent = crumb(visual, "chooser");
        visual.update(|window, cx| {
            window
                .within("directory-address")
                .click(("directory-crumb", parent), cx)
        });
        settle(visual, |_, cx| picker.read(cx).children_stop.is_some());
        settle(visual, |window, _| {
            window
                .within("popup-menu")
                .try_find(1usize)
                .is_some_and(|item| {
                    item.role() == Some(Role::MenuItem) && item.label() == Some("Beta")
                })
        });
        visual.update(|window, cx| {
            assert_eq!(picker.read(cx).history, history);
            assert!(!picker.read(cx).editing_address);
            window.within("popup-menu").click(1usize, cx);
        });
        wait(visual, &picker);
        visual.update(|_, cx| {
            assert_eq!(
                picker.read(cx).listing.as_ref().unwrap().directory.path,
                path.join("Beta").canonicalize().unwrap().to_str().unwrap()
            );
            assert_eq!(picker.read(cx).history.len(), history.len() + 1);
        });
        assert!(observed.browses.lock().unwrap().iter().any(|command| {
            matches!(command, Command::BrowseFiles { directory: Some(directory), after: Some(_) }
                if directory == path.canonicalize().unwrap().to_str().unwrap())
        }));
        dismiss(visual, &picker);

        let paused = Paused::new(fixture.transport.clone());
        paused.hold.store(false, Ordering::SeqCst);
        let (picker, _, visual) = show(
            cx,
            Arc::new(Client::new(paused.clone())),
            path.join("Alpha").to_str().unwrap().into(),
            Mode::Directory,
            false,
        );
        wait(visual, &picker);
        let history = visual.update(|_, cx| picker.read(cx).history.clone());
        paused.hold.store(true, Ordering::SeqCst);
        let parent = crumb(visual, "chooser");
        visual.update(|window, cx| {
            window
                .within("directory-address")
                .click(("directory-crumb", parent), cx)
        });
        settle(visual, |_, _| paused.entered.load(Ordering::SeqCst) == 1);
        let stop = visual.update(|_, cx| picker.read(cx).children_stop.clone().unwrap());
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(stop.is_cancelled());
        visual.update(|_, cx| {
            assert_eq!(picker.read(cx).history, history);
            assert!(!picker.read(cx).busy);
            assert!(picker.read(cx).error.is_none());
        });
        dismiss(visual, &picker);
        fixture.close();
    }
}

fn name_field(window: &mut Window) -> gpui_kit::test::ElementSnapshot {
    let mut fields = window.within("dialog-1").find_all_by_role(Role::TextInput);
    assert_eq!(
        fields.len(),
        1,
        "name form must expose one native text input"
    );
    fields.pop().unwrap()
}

fn name_input(visual: &mut VisualTestContext, value: &str) {
    visual.update(|window, cx| {
        let id = name_field(window).path().last().unwrap().clone();
        window.within("dialog-1").click(id, cx);
    });
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_input(value);
    visual.run_until_parked();
}

fn go_to_field(window: &mut Window) -> gpui_kit::test::ElementSnapshot {
    let target = window.find("directory-go-to-target");
    let list: ElementId = "list-state".into();
    let end = target
        .path()
        .iter()
        .rposition(|item| item == &list)
        .expect("go-to target must belong to a Kit list");
    let scope = &target.path()[..=end];
    let mut fields: Vec<_> = window
        .find_all_by_role(Role::TextInput)
        .into_iter()
        .filter(|field| field.path().starts_with(scope))
        .collect();
    assert_eq!(fields.len(), 1, "go-to list must expose one native input");
    fields.pop().unwrap()
}

#[gpui::test]
fn name_forms_preserve_drafts_and_cancel(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::directory(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let path = fixture.directory.path().join("chooser");
        std::fs::create_dir(&path).unwrap();
        let (picker, _, visual) = show(
            cx,
            Arc::new(Client::new(fixture.transport.clone())),
            path.to_str().unwrap().into(),
            Mode::Directory,
            false,
        );
        wait(visual, &picker);
        visual.update(|window, cx| {
            picker.update(cx, |picker, cx| {
                picker.dispatch(&actions::Dispatch(actions::Operation::Create), window, cx);
            })
        });
        settle(visual, |window, _| {
            window.try_find("directory-name-save").is_some()
        });
        name_input(visual, "../invalid");
        visual.update(|window, cx| window.click("directory-name-save", cx));
        settle(visual, |window, _| {
            window.try_find("directory-name-save").is_some()
        });
        visual.update(|window, _| {
            assert_eq!(name_field(window).value(), Some("../invalid"));
        });
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !matches!(request.command, Command::ManageFiles(_)))
        );
        name_input(visual, "cancelled");
        let cancel = visual
            .debug_bounds("directory-name-cancel")
            .unwrap()
            .center();
        visual.simulate_click(cancel, Modifiers::default());
        assert!(!path.join("cancelled").exists());
        visual.update(|window, cx| {
            picker.update(cx, |picker, cx| {
                picker.dispatch(&actions::Dispatch(actions::Operation::Create), window, cx);
            })
        });
        settle(visual, |window, _| {
            window.try_find("directory-name-save").is_some()
        });
        name_input(visual, "created");
        visual.update(|window, cx| window.click("directory-name-save", cx));
        wait(visual, &picker);
        assert!(path.join("created").is_dir());
        visual.update(|window, cx| {
            picker.update(cx, |picker, cx| {
                let index = picker
                    .entries()
                    .iter()
                    .position(|entry| entry.name == "created")
                    .unwrap();
                picker.choose(index, false, window, cx);
                picker.dispatch(&actions::Dispatch(actions::Operation::Rename), window, cx);
            })
        });
        settle(visual, |window, _| {
            window.try_find("directory-name-save").is_some()
        });
        name_input(visual, "renamed");
        visual.simulate_keystrokes("enter");
        wait(visual, &picker);
        assert!(path.join("renamed").is_dir());
        assert!(!path.join("created").exists());
        visual.update(|_, cx| assert!(!picker.read(cx).closed));
        dismiss(visual, &picker);
        fixture.close();
    }
}

#[gpui::test]
fn go_to_preserves_navigation_until_success(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::directory(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let path = fixture.directory.path().join("chooser");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("folder")).unwrap();
        std::fs::write(path.join("file.txt"), "contents").unwrap();
        let path = path.canonicalize().unwrap();
        let (picker, confirmations, visual) = show(
            cx,
            Arc::new(Client::new(fixture.transport.clone())),
            path.to_str().unwrap().into(),
            Mode::Directory,
            false,
        );
        wait(visual, &picker);
        let folder = visual.update(|_, cx| index(&picker, "folder", cx));
        click_row(visual, &picker, folder, Modifiers::default());
        let browses = || {
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(request.command, Command::BrowseFiles { .. }))
                .count()
        };
        let initial = browses();
        visual.update(|window, cx| {
            window.render_frame(cx);
            window.click("directory_go_to", cx);
        });
        settle(visual, |window, _| {
            window.try_find("directory-go-to-target").is_some()
        });
        let missing = path.join("missing").to_str().unwrap().to_owned();
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input(&missing);
        visual.run_until_parked();
        assert_eq!(browses(), initial, "typing must not browse the Node");
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert_eq!(picker.read(cx).selected, BTreeSet::from([folder]));
            window.click("directory-go-to-target", cx);
        });
        settle(visual, |_, cx| {
            picker.read(cx).error == Some("files_directory_failed") && !picker.read(cx).busy
        });
        visual.update(|window, cx| {
            assert_eq!(go_to_field(window).value(), Some(missing.as_str()));
            assert_eq!(
                picker.read(cx).listing.as_ref().unwrap().directory.path,
                path.to_str().unwrap()
            );
            assert_eq!(picker.read(cx).selected, BTreeSet::from([folder]));
            assert_eq!(picker.read(cx).history.len(), 1);
            assert_eq!(confirmations.load(Ordering::SeqCst), 0);
        });
        let file = path.join("file.txt").to_str().unwrap().to_owned();
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input(&file);
        visual.simulate_keystrokes("enter");
        settle(visual, |_, cx| {
            browses() >= initial + 2 && !picker.read(cx).busy
        });
        visual.update(|window, cx| {
            assert!(picker.read(cx).go_to.is_some());
            assert_eq!(picker.read(cx).selected, BTreeSet::from([folder]));
            assert_eq!(
                picker.read(cx).listing.as_ref().unwrap().directory.path,
                path.to_str().unwrap()
            );
            assert_eq!(go_to_field(window).value(), Some(file.as_str()));
        });
        visual.simulate_keystrokes("escape");
        settle(visual, |window, cx| {
            picker.read(cx).go_to.is_none() && picker.read(cx).focus.is_focused(window)
        });
        visual.update(|window, cx| {
            assert!(!picker.read(cx).closed);
            assert_eq!(picker.read(cx).history.len(), 1);
            assert!(picker.read(cx).error.is_none());
            assert_eq!(picker.read(cx).selected, BTreeSet::from([folder]));
            window.render_frame(cx);
            window.click("directory-confirm", cx);
            assert_eq!(confirmations.load(Ordering::SeqCst), 1);
            window.click("directory_go_to", cx);
        });
        settle(visual, |window, _| {
            window.try_find("directory-go-to-target").is_some()
        });
        let destination = path.join("folder").to_str().unwrap().to_owned();
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input(&destination);
        visual.simulate_keystrokes("enter");
        settle(visual, |_, cx| {
            picker.read(cx).go_to.is_none()
                && picker.read(cx).listing.as_ref().unwrap().directory.path == destination
        });
        visual.update(|window, cx| {
            assert!(!picker.read(cx).closed);
            assert!(picker.read(cx).selected.is_empty());
            assert_eq!(picker.read(cx).history.len(), 2);
            assert!(picker.read(cx).focus.is_focused(window));
            assert!(picker.read(cx).error.is_none());
            assert_eq!(confirmations.load(Ordering::SeqCst), 1);
            assert!(window.try_find("directory-go-to-target").is_none());
            assert!(window.try_find("directory-confirm").is_some());
        });
        assert!(browses() >= initial + 3);
        dismiss(visual, &picker);
        fixture.close();
    }
}
