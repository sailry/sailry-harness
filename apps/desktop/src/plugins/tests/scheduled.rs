use super::*;
use sailry_protocol::{Output, plugin};
use serde_json::{Value, json};

fn install(fixture: &Fixture) -> plugin::Info {
    let Output::Providers(mut providers) = fixture.execute(Command::ListProviders) else {
        panic!("providers expected")
    };
    let mut provider = providers.remove(0);
    let revision = provider.revision;
    provider.models[0].reasoning = true;
    provider.models[0].efforts = vec![
        sailry_protocol::Effort::Low,
        sailry_protocol::Effort::Medium,
        sailry_protocol::Effort::High,
    ];
    provider.models[0].default_effort = sailry_protocol::Effort::High;
    fixture.execute(Command::PutProvider {
        provider,
        expected_revision: revision,
    });
    let root = fixture.directory.path().join("project/tasks");
    fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/scheduled-tasks"),
        &root,
    );
    for file in [
        "dev.sailry.platform/desktop/view.js",
        "dev.sailry.platform/desktop/editor.js",
    ] {
        let path = root.join(file);
        let mut source = std::fs::read_to_string(&path).unwrap().replace("{Button,", "{Button as KitButton,")
            .replace(".id(\"tasks-controls\")", ".id(\"tasks-controls\").relative().child(Bounds.new(view.busy || view.pending ? 'tasks-busy' : 'tasks-ready'))")
            .replace("new Tab().label(text.tasks)", "new Tab().child(Anchor.new('tasks-tab-tasks').child(div().child(text.tasks)))")
            .replace("new Tab().label(text.runs)", "new Tab().child(Anchor.new('tasks-tab-runs').child(div().child(text.runs)))")
            .replace(".child(new Switch(`task-enabled-${item.id}`)", ".child(Anchor.new(`task-enabled-${item.id}`).child(new Switch(`task-enabled-${item.id}`)")
            .replace("view.perform(\"save\",{...item,enabled},cx))))", "view.perform(\"save\",{...item,enabled},cx)))))");
        for id in [
            "scheduled-tasks-page",
            "tasks-content",
            "tasks-heading",
            "tasks-description",
            "tasks-toolbar",
        ] {
            source = source.replace(
                &format!(".id(\"{id}\")"),
                &format!(".id(\"{id}\").relative().child(Bounds.new('{id}'))"),
            );
        }
        if file == "dev.sailry.platform/desktop/editor.js" {
            for label in [
                "name", "prompt", "schedule", "interval", "project", "model", "strength",
            ] {
                source = source.replace(&format!("new Field().label(text.{label})"),
                    &format!("new Field().label(text.{label}).relative().child(Bounds.new('task-{label}-field'))"));
            }
            source = source.replace(".id(\"task-editor\")", ".id(\"task-editor\").relative().child(Bounds.new('task-form')).child(Bounds.new(editing.draft.config ? `task-config-${editing.draft.config.effort}` : 'task-config-empty'))");
        }
        std::fs::write(path, format!("import {{Anchor, Bounds}} from 'sailry/test';\n{source}\nfunction Button(id) {{ const button = new KitButton(id); button.label = text => button.child(Anchor.new(id).child(div().child(text))); return button; }}")).unwrap();
    }
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "tasks".into(),
        name: "scheduled-tasks".into(),
        expected_revision: 1,
    }) else {
        panic!("plugin expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

fn call(fixture: &Fixture, package: &plugin::Info, handler: &str, input: Value) -> Value {
    let request = fixture
        .binding
        .client
        .prepare(Command::CallPlugin {
            handler: handler.into(),
            input,
        })
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: None,
            session: None,
        });
    let Output::PluginResult(result) = fixture
        .runtime
        .block_on(fixture.binding.client.execute(request))
        .unwrap()
    else {
        panic!("plugin result expected")
    };
    assert!(result.get("Ok").is_some(), "{result}");
    result["Ok"].clone()
}
fn items(fixture: &Fixture, package: &plugin::Info) -> Vec<Value> {
    call(fixture, package, "list", json!({}))["items"]
        .as_array()
        .unwrap()
        .clone()
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
    visual.simulate_window_resize(handle, size(px(1100.), px(1100.)));
    wait(visual, |cx| {
        panel.read(cx).connected
            && panel
                .read(cx)
                .metadata
                .read(cx)
                .entries
                .contains_key("scheduled-tasks")
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("scheduled-tasks-page")
    });
    (panel, visual)
}
fn select(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn choose_model(fixture: &Fixture, panel: &Entity<Panel>, visual: &mut VisualTestContext) {
    let model = format!("{}/fixture", fixture.session.config.provider);
    wait(visual, |cx| snapshot(panel, cx).contains(&model));
    click(visual, "task-model");
    click(visual, select(format!("composer-model-option-0-{model}")));
    wait(visual, |cx| {
        snapshot(panel, cx).contains("task-config-high")
    });
}

fn form_layout(panel: &Entity<Panel>, visual: &mut VisualTestContext) {
    let tree = visual.update(|_, cx| snapshot(panel, cx));
    assert_eq!(
        tree.lines()
            .filter(|line| line.trim_start().starts_with("Field ")
                && line.contains(":label(registered)"))
            .count(),
        7
    );
    let form = visual.debug_bounds("task-form").unwrap();
    let mut previous = form.top();
    let mut cursor = 0;
    // Field probes measure control regions. Label bands come from the actual
    // native form/row geometry, with typed labels verified in snapshot order.
    for (field, control) in [
        ("name", "field-1"),
        ("prompt", "field-2"),
        ("schedule", "task-timing-item-once"),
        ("project", "task-project"),
        ("model", "task-model"),
        ("strength", "task-strength"),
    ] {
        let schedule = control == "task-timing-item-once";
        let position = tree.find(&format!("task-{field}-field")).unwrap();
        assert!(position > cursor);
        cursor = position;
        let field = visual
            .debug_bounds(select(format!("task-{field}-field")))
            .unwrap();
        let control = visual.debug_bounds(control).unwrap();
        assert!(
            control.top() >= previous + px(20.),
            "form {form:?}, region {field:?}, control {control:?}"
        );
        if !schedule {
            assert_eq!(control.left(), field.left());
            assert_eq!(control.right(), field.right());
        }
        assert!(control.left() >= form.left() && control.right() <= form.right());
        previous = control.bottom();
    }
    let name = visual.debug_bounds("field-1").unwrap();
    for control in ["task-project", "task-model", "task-strength"] {
        let bounds = visual.debug_bounds(control).unwrap();
        assert_eq!(bounds.size.width, name.size.width);
        assert_eq!(bounds.left(), name.left());
    }
    let schedule = visual.debug_bounds("task-schedule-field").unwrap();
    let once = visual.debug_bounds("task-timing-item-once").unwrap();
    let every = visual.debug_bounds("task-timing-item-every").unwrap();
    assert!((once.size.width - every.size.width).abs() < px(1.));
    assert!(once.left() - schedule.left() < px(8.));
    assert!(schedule.right() - every.right() < px(8.));
    assert!(
        visual.debug_bounds("task-strength").unwrap().bottom()
            < visual.debug_bounds("task-save").unwrap().top()
    );
}

#[gpui::test]
fn manages_task_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "task-create");
        wait(visual, |cx| snapshot(&panel, cx).contains("task-editor"));
        click(visual, "task-save");
        toast(visual, "Enter a name and task");
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("task-editor")
                && !snapshot(&panel, cx).contains("task-error-required")
        }));
        click(visual, "field-1");
        visual.simulate_input("Scheduled review 中文");
        click(visual, "field-2");
        visual.simulate_input("Review the project");
        click(visual, "task-save");
        toast(visual, "Choose a model");
        choose_model(&fixture, &panel, visual);
        form_layout(&panel, visual);
        click(visual, "task-strength");
        visual.simulate_keystrokes("up up enter");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("task-config-low")
        });
        click(visual, "task-timing-item-every");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("task-interval-field")
        });
        click(visual, "task-save");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Scheduled review 中文")
                && !snapshot(&panel, cx).contains("task-editor")
        });
        let saved = items(&fixture, &package).remove(0);
        assert_eq!(saved["timing"]["kind"], "every");
        assert_eq!(saved["timing"]["data"]["interval_ms"], 3600000);
        assert!(saved["project"].is_null());
        assert_eq!(
            saved["config"]["provider"],
            fixture.session.config.provider.to_string()
        );
        assert_eq!(saved["config"]["model"], "fixture");
        assert_eq!(saved["config"]["effort"], "low");
        let Output::Snapshot(state) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert!(state.defaults.config.is_none());
        let id = saved["id"].as_str().unwrap();
        click(visual, select(format!("task-enabled-{id}")));
        wait(visual, |cx| snapshot(&panel, cx).contains("Paused"));
        assert_eq!(items(&fixture, &package)[0]["enabled"], false);
        assert!(visual.debug_bounds("queue-more-default").is_none());
        assert!(visual.debug_bounds("queue-pause-default").is_none());
        assert!(
            call(&fixture, &package, "list", json!({}))
                .get("queues")
                .is_none()
        );
        assert_eq!(items(&fixture, &package)[0]["queue"], "default");
        wait(visual, |cx| snapshot(&panel, cx).contains("tasks-ready"));
        click(visual, select(format!("task-delete-{id}")));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("task-delete-confirmation")
        });
        click(visual, "task-delete-confirm");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("task-delete-confirmation")
                && !snapshot(&panel, cx).contains("Scheduled review 中文")
        });
        assert!(items(&fixture, &package).is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn retains_drafts_and_requests(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        click(visual, "task-create");
        wait(visual, |cx| snapshot(&panel, cx).contains("task-editor"));
        click(visual, "field-1");
        visual.simulate_input("Original task");
        click(visual, "field-2");
        visual.simulate_input("Keep this prompt");
        choose_model(&fixture, &panel, visual);
        fixture.transport.mode.store(9, Ordering::SeqCst);
        click(visual, "task-save");
        toast(visual, "Result unknown · retry to check");
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("task-editor")
                && !snapshot(&panel, cx).contains("task-error-unknown")
        }));
        assert_eq!(items(&fixture, &package).len(), 1);
        click(visual, "task-save");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("task-editor")
                && snapshot(&panel, cx).contains("Original task")
        });
        let mut task = items(&fixture, &package).remove(0);
        click(
            visual,
            select(format!("task-edit-{}", task["id"].as_str().unwrap())),
        );
        wait(visual, |cx| snapshot(&panel, cx).contains("task-editor"));
        click(visual, "field-4");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Unsaved title");
        task["name"] = json!("Changed elsewhere");
        call(&fixture, &package, "save", task);
        visual.update(|window, cx| window.clear_notifications(cx));
        click(visual, "task-save");
        toast(visual, "Task changed · reopen to edit");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("task-error-conflict")
        }));
        assert!(visual.debug_bounds("field-4").is_some());
        click(visual, "field-4");
        visual.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Unsaved title"
        );
        assert_eq!(items(&fixture, &package)[0]["name"], "Changed elsewhere");
        click(visual, "task-cancel");
        let requests = fixture.transport.requests.lock().unwrap();
        let saves:Vec<_> = requests.iter().filter(|request| matches!(&request.command,Command::CallPlugin{handler,..} if handler=="save")).collect();
        assert_eq!(saves[0].id, saves[1].id);
        drop(requests);
        fixture.execute(Command::SetPluginEnabled {
            name: "scheduled-tasks".into(),
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
fn history_opens_the_original_node_session(cx: &mut TestAppContext) {
    use crate::{preview::Page, shell::Shell};
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let server = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_after(
                "Scheduled response".into(),
                Duration::ZERO,
            ));
        fixture.game_endpoint(&server.endpoint);
        let package = install(&fixture);
        let task = call(
            &fixture,
            &package,
            "save",
            json!({"revision":"0", "name":"History navigation", "prompt":"Run the review", "queue":"default", "project":null,"worktree":null,"config":fixture.session.config,"enabled":false,"timing":{"kind":"once","data":{"at_ms":chrono::Utc::now().timestamp_millis()+600000}}}),
        );
        let job = call(&fixture, &package, "run", json!({"id":task["id"]}));
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let mut binding = fixture.binding.clone();
            binding.worktree = None;
            let panel = cx.new(|cx| Panel::standalone(binding, cx));
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(window, cx);
                shell.page = Page::Plugin;
                Shell::observe_plugin_conversations(&panel, window, cx);
                shell
            });
            owner = Some((panel.clone(), shell.clone()));
            let routed = cx.new(|cx| {
                cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                cx.observe(&shell, |_, _, cx| cx.notify()).detach();
                Routed { panel, shell }
            });
            Root::new(routed, window, cx)
        });
        let (panel, shell) = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1400.), px(1100.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx);
                // This harness renders the captured Panel until a session opens,
                // so start the Shell's ordinary per-Node activity subscriptions.
                shell.bind_activity(window, cx);
            })
        });
        wait(visual, |cx| {
            panel.read(cx).ready_for(&package.summary.reference(), cx)
        });
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("tasks-items"));
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            fixture.node.id()
        );
        let button = select(format!("task-open-{}", job["id"].as_str().unwrap()));
        assert!(!visual.update(|_, cx| snapshot(&panel, cx)).contains(button));
        click(visual, "tasks-tab-runs");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("tasks-runs") && snapshot(&panel, cx).contains(button)
        });
        let history = call(&fixture, &package, "history", json!({}));
        let session: sailry_protocol::SessionId =
            serde_json::from_value(history["items"][0]["session"].clone()).unwrap();
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity_snapshot(fixture.node.id())
                .is_some_and(|snapshot| snapshot.sessions.iter().any(|entry| entry.id == session))
        });
        click(visual, button);
        wait(visual, |cx| {
            shell.read(cx).page == Page::Conversation
                && shell
                    .read(cx)
                    .current_chat()
                    .is_some_and(|chat| chat.read(cx).session() == Some(session))
        });
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .current_chat()
                .unwrap()
                .read(cx)
                .binding()
                .client
                .target()),
            fixture.node.id()
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

struct Routed {
    panel: Entity<Panel>,
    shell: Entity<crate::shell::Shell>,
}

impl Render for Routed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.shell.read(cx).page == crate::preview::Page::Conversation {
            self.shell.clone().into_any_element()
        } else {
            self.panel.clone().into_any_element()
        }
    }
}

fn layout(visual: &mut VisualTestContext, width: f32) {
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(width), px(940.)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let page = visual.debug_bounds("scheduled-tasks-page").unwrap();
    let body = visual.debug_bounds("tasks-content").unwrap();
    let heading = visual.debug_bounds("tasks-heading").unwrap();
    let description = visual.debug_bounds("tasks-description").unwrap();
    let toolbar = visual.debug_bounds("tasks-toolbar").unwrap();
    assert!(body.size.width <= px(800.));
    assert!(body.left() >= page.left() && body.right() <= page.right());
    assert_eq!(body.center().x, page.center().x);
    if width > 800. {
        assert_eq!(body.size.width, px(800.));
    }
    assert!(heading.bottom() <= description.top());
    assert!(description.bottom() <= toolbar.top());
    for selector in ["tasks-tab-tasks", "tasks-tab-runs", "task-create"] {
        let control = visual.debug_bounds(selector).unwrap();
        assert!(
            control.left() >= body.left() && control.right() <= body.right(),
            "{selector} must stay inside the body"
        );
    }
    assert!(
        visual.debug_bounds("tasks-tab-runs").unwrap().right()
            < visual.debug_bounds("task-create").unwrap().left()
    );
    assert!(visual.debug_bounds("tasks-refresh").is_none());
}

#[gpui::test]
fn centers_the_page_and_switches_native_tabs(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        wait(visual, |cx| snapshot(&panel, cx).contains("tasks-empty"));
        layout(visual, 1440.);
        assert!(visual.debug_bounds("empty-tasks-empty").is_some());
        empty_card(visual, "tasks-empty");
        click(visual, "tasks-tab-runs");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("tasks-runs-empty")
        });
        assert!(visual.debug_bounds("empty-tasks-runs-empty").is_some());
        empty_card(visual, "tasks-runs-empty");
        assert!(visual.debug_bounds("empty-tasks-empty").is_none());
        click(visual, "tasks-tab-tasks");
        wait(visual, |cx| snapshot(&panel, cx).contains("tasks-items"));
        let saved = call(
            &fixture,
            &package,
            "save",
            json!({"revision":"0","name":"Card layout", "prompt":"A retained prompt", "queue":"default", "project":null,"worktree":null,"config":fixture.session.config,"enabled":false,"timing":{"kind":"once","data":{"at_ms":chrono::Utc::now().timestamp_millis()+600000}}}),
        );
        let selector = select(format!("task-card-{}", saved["id"].as_str().unwrap()));
        wait(visual, |cx| snapshot(&panel, cx).contains(selector));
        assert!(visual.debug_bounds("empty-tasks-empty").is_none());
        let card = visual.debug_bounds(selector).unwrap();
        let body = visual.debug_bounds("tasks-content").unwrap();
        assert!(card.left() >= body.left() && card.right() <= body.right());
        card_row(visual, saved["id"].as_str().unwrap());
        click(visual, "tasks-tab-runs");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("tasks-runs-empty")
                && !snapshot(&panel, cx).contains(selector)
        });
        click(visual, "tasks-tab-tasks");
        wait(visual, |cx| snapshot(&panel, cx).contains(selector));
        layout(visual, 640.);
        let card = visual.debug_bounds(selector).unwrap();
        let body = visual.debug_bounds("tasks-content").unwrap();
        assert!(card.left() >= body.left() && card.right() <= body.right());
        card_row(visual, saved["id"].as_str().unwrap());
        assert_eq!(items(&fixture, &package).len(), 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

fn card_row(visual: &mut VisualTestContext, id: &str) {
    let row = visual
        .debug_bounds(select(format!("task-row-{id}")))
        .unwrap();
    assert!(row.size.height >= px(64.) && row.size.height <= px(72.));
    let mut right = row.left();
    for part in ["summary", "status", "leading", "actions"] {
        let column = visual
            .debug_bounds(select(format!("task-{part}-{id}")))
            .unwrap();
        assert!(
            column.left() >= right && column.right() <= row.right(),
            "{part}: {column:?}, row: {row:?}"
        );
        assert!((column.center().y - row.center().y).abs() < px(1.));
        right = column.right();
    }
    let summary = visual
        .debug_bounds(select(format!("task-summary-{id}")))
        .unwrap();
    let title = visual
        .debug_bounds(select(format!("task-summary-{id}-title")))
        .unwrap();
    let project = visual
        .debug_bounds(select(format!("task-summary-{id}-subtitle")))
        .unwrap();
    assert!(title.bottom() < project.top());
    assert_eq!(title.left(), project.left());
    assert!(title.right() <= summary.right() && project.right() <= summary.right());
    assert_eq!(
        visual
            .debug_bounds(select(format!("task-status-{id}")))
            .unwrap()
            .size
            .width,
        px(128.)
    );
    assert_eq!(
        visual
            .debug_bounds(select(format!("task-leading-{id}")))
            .unwrap()
            .size
            .width,
        px(48.)
    );
    let actions = visual
        .debug_bounds(select(format!("task-actions-{id}")))
        .unwrap();
    let mut right = actions.left();
    for action in ["run", "edit", "delete"] {
        let button = visual
            .debug_bounds(select(format!("task-{action}-{id}")))
            .unwrap();
        assert!(button.left() >= right && button.right() <= actions.right());
        assert!(button.size.width <= px(32.));
        right = button.right();
    }
}

fn empty_card(visual: &mut VisualTestContext, id: &str) {
    let card = visual
        .debug_bounds(select(format!("empty-card-{id}")))
        .unwrap();
    let toolbar = visual.debug_bounds("tasks-toolbar").unwrap();
    let icon = visual
        .debug_bounds(select(format!("empty-icon-{id}")))
        .unwrap();
    let title = visual
        .debug_bounds(select(format!("empty-title-{id}")))
        .unwrap();
    assert_eq!(card.size.width, toolbar.size.width);
    assert!(card.top() >= toolbar.bottom());
    assert!(card.size.height < px(180.));
    assert_eq!(icon.size, size(px(48.), px(48.)));
    assert!(title.size.height <= px(24.));
}
