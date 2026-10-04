use super::*;
use sailry_protocol::{Output, plugin};
use serde_json::{Value, json};

fn install(fixture: &Fixture) -> plugin::Info {
    let root = fixture.directory.path().join("project/reminders");
    super::super::fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/reminders"),
        &root,
    );
    // Give native control labels layout anchors; no behavior or callback changes.
    for file in [
        "dev.sailry.platform/desktop/view.js",
        "dev.sailry.platform/desktop/editor.js",
    ] {
        let path = root.join(file);
        let source = std::fs::read_to_string(&path).unwrap();
        let mut source = source.replace("{ Button,", "{ Button as KitButton,")
            .replace(".label(text.all)", ".child(Anchor.new('reminders-all').child(div().child(text.all)))")
            .replace(".label(text.pending)", ".child(Anchor.new('reminders-pending').child(div().child(text.pending)))")
            .replace(".label(text.completed)", ".child(Anchor.new('reminders-completed').child(div().child(text.completed)))")
            .replace(".child(new Checkbox(`reminder-complete-${item.id}`)", ".child(Anchor.new(`reminder-complete-${item.id}`).child(new Checkbox(`reminder-complete-${item.id}`)")
            .replace("view.perform(\"save\", {...item, completed:checked}, cx)))", "view.perform(\"save\", {...item, completed:checked}, cx))))")
            .replace(".child(new Checkbox(\"reminder-timed\")", ".child(Anchor.new('reminder-timed').child(new Checkbox(\"reminder-timed\")")
            .replace("editing.timed = checked; cx.notify(); }))", "editing.timed = checked; cx.notify(); })))");
        if file == "dev.sailry.platform/desktop/editor.js" {
            for (label, selector) in [
                ("title", "reminder-title-field"),
                ("message", "reminder-note-field"),
                ("project", "reminder-project-field"),
            ] {
                source = source.replace(
                    &format!("new Field().label(text.{label})"),
                    &format!(
                        "new Field().label(text.{label}).relative().child(Bounds.new('{selector}'))"
                    ),
                );
            }
        }
        for id in [
            "reminders-page",
            "reminders-content",
            "reminders-heading",
            "reminders-description",
            "reminders-toolbar",
            "reminder-fields",
        ] {
            source = source.replace(
                &format!(".id(\"{id}\")"),
                &format!(".id(\"{id}\").relative().child(Bounds.new('{id}'))"),
            );
        }
        let source = format!(
            "import {{ Anchor, Bounds }} from 'sailry/test';\n{source}\nfunction Button(id) {{ const button = new KitButton(id); button.label = text => button.child(Anchor.new(id).child(div().child(text))); return button; }}\n"
        );
        std::fs::write(path, source).unwrap();
    }
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "reminders".into(),
        name: "reminders".into(),
        expected_revision: 1,
    }) else {
        panic!("plugin expected");
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

fn items(fixture: &Fixture, package: &plugin::Info) -> Vec<Value> {
    let context = plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: None,
        session: None,
    };
    let request = fixture
        .binding
        .client
        .prepare(Command::CallPlugin {
            handler: "list".into(),
            input: json!({}),
        })
        .with_plugin(context);
    let Output::PluginResult(result) = fixture
        .runtime
        .block_on(fixture.binding.client.execute(request))
        .unwrap()
    else {
        panic!("plugin result expected");
    };
    result["Ok"]["items"].as_array().unwrap().clone()
}

fn mount<'a>(
    fixture: &Fixture,
    package: &plugin::Info,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let mut binding = fixture.binding.clone();
        binding.worktree = None;
        let panel = cx.new(|cx| Panel::standalone(binding, cx));
        owner = Some(panel.clone());
        let frame = cx.new(|cx| {
            cx.observe(&panel, |_, _, cx| cx.notify()).detach();
            Panel::observe_notifications(&panel, window, cx);
            Harness(panel)
        });
        Root::new(frame, window, cx)
    });
    let panel = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1000.), px(820.)));
    wait(visual, |cx| {
        panel.read(cx).connected
            && panel
                .read(cx)
                .metadata
                .read(cx)
                .entries
                .contains_key("reminders")
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| snapshot(&panel, cx).contains("reminders-page"));
    (panel, visual)
}

fn select(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn save(fixture: &Fixture, package: &plugin::Info, input: Value) -> Value {
    let context = plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: None,
        session: None,
    };
    let request = fixture
        .binding
        .client
        .prepare(Command::CallPlugin {
            handler: "save".into(),
            input,
        })
        .with_plugin(context);
    let Output::PluginResult(result) = fixture
        .runtime
        .block_on(fixture.binding.client.execute(request))
        .unwrap()
    else {
        panic!("plugin result expected");
    };
    assert!(result.get("Ok").is_some(), "{result}");
    result["Ok"].clone()
}

#[gpui::test]
fn associates_and_filters_projects_with_native_selects(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        let project = fixture.binding.project.unwrap();
        wait(visual, |cx| {
            snapshot(&panel, cx)
                .lines()
                .any(|line| line.contains("SelectField") && line.contains(&project.to_string()))
        });
        click(visual, "reminder-create");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-editor")
        });
        form_layout(&panel, visual);
        click(visual, "field-1");
        visual.simulate_input("Project reminder");
        click(visual, "reminder-project");
        visual.simulate_keystrokes("down enter");
        click(visual, "reminder-save");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Project reminder")
                && !snapshot(&panel, cx).contains("reminder-editor")
        });
        let associated = items(&fixture, &package).remove(0);
        assert_eq!(associated["project"], json!(project));
        let associated_card = select(format!(
            "reminder-card-{}",
            associated["id"].as_str().unwrap()
        ));
        click(visual, "reminder-create");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-editor")
        });
        click(visual, "field-3");
        visual.simulate_input("Unassigned reminder");
        click(visual, "reminder-save");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Unassigned reminder")
                && !snapshot(&panel, cx).contains("reminder-editor")
        });
        let records = items(&fixture, &package);
        let unassigned = records
            .iter()
            .find(|item| item["title"] == "Unassigned reminder")
            .unwrap();
        assert_eq!(unassigned["project"], Value::Null);
        let unassigned_card = select(format!(
            "reminder-card-{}",
            unassigned["id"].as_str().unwrap()
        ));
        click(visual, "reminders-project-filter");
        visual.simulate_keystrokes("down down enter");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(associated_card)
                && !snapshot(&panel, cx).contains(unassigned_card)
        });
        click(visual, "reminders-project-filter");
        visual.simulate_keystrokes("up enter");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains(associated_card)
                && snapshot(&panel, cx).contains(unassigned_card)
        });
        click(visual, "reminders-project-filter");
        visual.simulate_keystrokes("up enter");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(associated_card)
                && snapshot(&panel, cx).contains(unassigned_card)
        });
        assert_eq!(items(&fixture, &package).len(), 2);
        click(
            visual,
            select(format!(
                "reminder-edit-{}",
                associated["id"].as_str().unwrap()
            )),
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-editor")
        });
        click(visual, "reminder-project");
        visual.simulate_keystrokes("up enter");
        click(visual, "reminder-save");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("reminder-editor")
        });
        assert!(
            items(&fixture, &package)
                .iter()
                .all(|item| item["project"].is_null())
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn delivery_preserves_conflicting_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        let saved = save(
            &fixture,
            &package,
            json!({"revision":"0","title":"Background reminder", "message":"Original note", "project":null,"completed":false,"due_ms":chrono::Utc::now().timestamp_millis()+250}),
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Background reminder")
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("Notified"));
        let id = saved["id"].as_str().unwrap();
        click(visual, select(format!("reminder-edit-{id}")));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-editor")
        });
        click(visual, "field-1");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Unsaved draft");
        let mut changed = items(&fixture, &package).remove(0);
        changed["title"] = json!("Changed elsewhere");
        save(&fixture, &package, changed);
        click(visual, "reminder-save");
        toast(visual, "Reminder changed. Reopen to edit");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("reminder-error-conflict")
        }));
        assert!(visual.debug_bounds("field-1").is_some());
        click(visual, "field-1");
        visual.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Unsaved draft"
        );
        assert_eq!(items(&fixture, &package)[0]["title"], "Changed elsewhere");
        click(visual, "reminder-cancel");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Changed elsewhere")
                && !snapshot(&panel, cx).contains("reminder-editor")
        });
        fixture.execute(Command::SetPluginEnabled {
            name: "reminders".into(),
            expected_revision: 2,
            enabled: false,
        });
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn edits_and_deletes_with_native_fields(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "reminder-create");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-editor")
        });
        click(visual, "reminder-save");
        toast(visual, "Enter a title");
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("reminder-editor")
                && !snapshot(&panel, cx).contains("reminder-error-required")
        }));
        assert!(items(&fixture, &package).is_empty());
        click(visual, "field-1");
        visual.simulate_input("Reminder 中文");
        click(visual, "field-2");
        visual.simulate_input("Keep the note");
        click(visual, "reminder-timed");
        click(visual, "reminder-save");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Reminder 中文")
                && !snapshot(&panel, cx).contains("reminder-editor")
        });
        let mut saved = items(&fixture, &package);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["message"], "Keep the note");
        assert!(saved[0]["due_ms"].as_i64().unwrap() > chrono::Utc::now().timestamp_millis());
        let id = saved[0]["id"].as_str().unwrap().to_owned();
        click(visual, "reminders-pending");
        click(visual, select(format!("reminder-complete-{id}")));
        wait(visual, |cx| !snapshot(&panel, cx).contains("Reminder 中文"));
        click(visual, "reminders-completed");
        wait(visual, |cx| snapshot(&panel, cx).contains("Reminder 中文"));
        saved = items(&fixture, &package);
        assert_eq!(saved[0]["completed"], true);
        click(visual, select(format!("reminder-edit-{id}")));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-editor")
        });
        click(visual, "field-3");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Changed title");
        click(visual, "reminder-cancel");
        assert_eq!(items(&fixture, &package), saved);
        click(visual, select(format!("reminder-delete-{id}")));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminder-delete-confirmation")
        });
        click(visual, "reminder-delete-cancel");
        assert_eq!(items(&fixture, &package), saved);
        click(visual, select(format!("reminder-delete-{id}")));
        click(visual, "reminder-delete-confirm");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("reminder-delete-confirmation")
                && !snapshot(&panel, cx).contains("Reminder 中文")
        });
        assert!(items(&fixture, &package).is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn uncertain_save_can_be_retried_or_closed(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        for (index, retry) in [true, false].into_iter().enumerate() {
            click(visual, "reminder-create");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("reminder-editor")
            });
            click(visual, select(format!("field-{}", index * 2 + 1)));
            visual.simulate_input(if retry {
                "Retry this receipt"
            } else {
                "Close this draft"
            });
            fixture.transport.mode.store(9, Ordering::SeqCst);
            visual.update(|window, cx| window.clear_notifications(cx));
            crate::feedback::tests::settle(visual);
            assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
            click(visual, "reminder-save");
            toast(visual, "Result unconfirmed. Retry to check");
            assert!(panel.read_with(visual, |_, cx| {
                snapshot(&panel, cx).contains("reminder-editor")
                    && !snapshot(&panel, cx).contains("reminder-error-unknown")
            }));
            assert_eq!(items(&fixture, &package).len(), index + 1);
            click(
                visual,
                if retry {
                    "reminder-save"
                } else {
                    "reminder-cancel"
                },
            );
            wait(visual, |cx| {
                !snapshot(&panel, cx).contains("reminder-editor")
            });
            assert_eq!(items(&fixture, &package).len(), index + 1);
        }
        let requests = fixture.transport.requests.lock().unwrap();
        let saves: Vec<_> = requests.iter().filter(|request| matches!(&request.command, Command::CallPlugin {handler,..} if handler=="save")).collect();
        assert_eq!(saves.len(), 3);
        assert_eq!(saves[0].id, saves[1].id);
        assert_ne!(saves[1].id, saves[2].id);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

fn layout(visual: &mut VisualTestContext, width: f32) {
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(width), px(940.)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let page = visual.debug_bounds("reminders-page").unwrap();
    let body = visual.debug_bounds("reminders-content").unwrap();
    let heading = visual.debug_bounds("reminders-heading").unwrap();
    let description = visual.debug_bounds("reminders-description").unwrap();
    let toolbar = visual.debug_bounds("reminders-toolbar").unwrap();
    assert!(body.size.width <= px(800.));
    assert!(body.left() >= page.left() && body.right() <= page.right());
    assert_eq!(body.center().x, page.center().x);
    if width > 800. {
        assert_eq!(body.size.width, px(800.));
    }
    assert!(heading.bottom() <= description.top());
    assert!(description.bottom() <= toolbar.top());
    for selector in [
        "reminders-all",
        "reminders-pending",
        "reminders-completed",
        "reminders-project-filter",
        "reminder-create",
    ] {
        let control = visual.debug_bounds(selector).unwrap();
        assert!(
            control.left() >= body.left() && control.right() <= body.right(),
            "{selector} must stay inside the body"
        );
        assert!(
            (control.center().y - toolbar.center().y).abs() < px(1.),
            "{selector} must be vertically centered: control={control:?}, toolbar={toolbar:?}"
        );
    }
    assert!(
        visual.debug_bounds("reminders-all").unwrap().right()
            < visual.debug_bounds("reminders-pending").unwrap().left()
    );
    assert!(
        visual.debug_bounds("reminders-pending").unwrap().right()
            < visual.debug_bounds("reminders-completed").unwrap().left()
    );
    assert!(
        visual.debug_bounds("reminders-completed").unwrap().right()
            < visual
                .debug_bounds("reminders-project-filter")
                .unwrap()
                .left()
    );
    assert!(visual.debug_bounds("reminders-refresh").is_none());
}

#[gpui::test]
fn centers_the_page_and_filters_native_cards(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminders-empty")
        });
        layout(visual, 1440.);
        assert!(visual.debug_bounds("empty-reminders-empty").is_some());
        empty_card(visual);
        let saved = save(
            &fixture,
            &package,
            json!({"revision":"0","title":"Card layout", "message":"A retained note", "project":null,"completed":false,"due_ms":null}),
        );
        let selector = select(format!("reminder-card-{}", saved["id"].as_str().unwrap()));
        wait(visual, |cx| snapshot(&panel, cx).contains(selector));
        assert!(visual.debug_bounds("empty-reminders-empty").is_none());
        let card = visual.debug_bounds(selector).unwrap();
        let body = visual.debug_bounds("reminders-content").unwrap();
        assert!(card.left() >= body.left() && card.right() <= body.right());
        card_row(visual, saved["id"].as_str().unwrap());
        click(visual, "reminders-completed");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("reminders-empty")
                && !snapshot(&panel, cx).contains(selector)
        });
        assert!(visual.debug_bounds("empty-reminders-empty").is_some());
        empty_card(visual);
        let completed = save(
            &fixture,
            &package,
            json!({"revision":"0","title":"Completed layout", "message":"A completed note", "project":null,"completed":true,"due_ms":null}),
        );
        let completed_selector = select(format!(
            "reminder-card-{}",
            completed["id"].as_str().unwrap()
        ));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(completed_selector)
        });
        assert!(!panel.read_with(visual, |_, cx| snapshot(&panel, cx).contains(selector)));
        click(visual, "reminders-pending");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(selector)
                && !snapshot(&panel, cx).contains(completed_selector)
        });
        click(visual, "reminders-all");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(selector)
                && snapshot(&panel, cx).contains(completed_selector)
        });
        layout(visual, 640.);
        let card = visual.debug_bounds(selector).unwrap();
        let body = visual.debug_bounds("reminders-content").unwrap();
        assert!(card.left() >= body.left() && card.right() <= body.right());
        card_row(visual, saved["id"].as_str().unwrap());
        assert_eq!(items(&fixture, &package).len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

fn card_row(visual: &mut VisualTestContext, id: &str) {
    let row = visual
        .debug_bounds(select(format!("reminder-row-{id}")))
        .unwrap();
    assert!(row.size.height >= px(64.) && row.size.height <= px(72.));
    let mut right = row.left();
    for part in ["summary", "time", "actions"] {
        let column = visual
            .debug_bounds(select(format!("reminder-{part}-{id}")))
            .unwrap();
        assert!(
            column.left() >= right && column.right() <= row.right(),
            "{part}: {column:?}, row: {row:?}"
        );
        assert!((column.center().y - row.center().y).abs() < px(1.));
        right = column.right();
    }
    let summary = visual
        .debug_bounds(select(format!("reminder-summary-{id}")))
        .unwrap();
    let title = visual
        .debug_bounds(select(format!("reminder-summary-{id}-title")))
        .unwrap();
    let project = visual
        .debug_bounds(select(format!("reminder-summary-{id}-subtitle")))
        .unwrap();
    let checkbox = visual
        .debug_bounds(select(format!("reminder-complete-{id}")))
        .unwrap();
    assert!(checkbox.left() >= summary.left() && checkbox.right() < title.left());
    assert!((checkbox.center().y - summary.center().y).abs() < px(1.));
    assert!(title.bottom() < project.top());
    assert_eq!(title.left(), project.left());
    assert!(title.right() <= summary.right() && project.right() <= summary.right());
    assert_eq!(
        visual
            .debug_bounds(select(format!("reminder-time-{id}")))
            .unwrap()
            .size
            .width,
        px(192.)
    );
}

fn empty_card(visual: &mut VisualTestContext) {
    let card = visual.debug_bounds("empty-card-reminders-empty").unwrap();
    let toolbar = visual.debug_bounds("reminders-toolbar").unwrap();
    let icon = visual.debug_bounds("empty-icon-reminders-empty").unwrap();
    let title = visual.debug_bounds("empty-title-reminders-empty").unwrap();
    assert_eq!(card.size.width, toolbar.size.width);
    assert!(card.top() >= toolbar.bottom());
    assert!(card.size.height < px(180.));
    assert_eq!(icon.size, size(px(48.), px(48.)));
    assert!(title.size.height <= px(24.));
}

fn form_layout(panel: &Entity<Panel>, visual: &mut VisualTestContext) {
    let tree = visual.update(|_, cx| snapshot(panel, cx));
    assert_eq!(
        tree.lines()
            .filter(|line| line.trim_start().starts_with("Field ")
                && line.contains(":label(registered)"))
            .count(),
        3
    );
    let form = visual.debug_bounds("reminder-fields").unwrap();
    let mut previous = form.top();
    let mut cursor = 0;
    // A Field child is in its control region, not its label/root. Measure the
    // whole native form and each control; typed labels remain in snapshot order.
    for (field, control) in [
        ("reminder-title-field", "field-1"),
        ("reminder-note-field", "field-2"),
        ("reminder-project-field", "reminder-project"),
    ] {
        let position = tree.find(field).unwrap();
        assert!(position > cursor);
        cursor = position;
        let field = visual.debug_bounds(field).unwrap();
        let control = visual.debug_bounds(control).unwrap();
        assert!(
            control.top() >= previous + px(20.),
            "form {form:?}, region {field:?}, control {control:?}"
        );
        assert_eq!(control.left(), field.left());
        assert_eq!(control.right(), field.right());
        assert!(control.left() >= form.left() && control.right() <= form.right());
        previous = control.bottom();
    }
    let title = visual.debug_bounds("field-1").unwrap();
    let project = visual.debug_bounds("reminder-project").unwrap();
    assert_eq!(project.size.width, title.size.width);
    assert!(visual.debug_bounds("field-2").unwrap().top() > title.bottom());
    assert!(project.bottom() < visual.debug_bounds("reminder-save").unwrap().top());
}
