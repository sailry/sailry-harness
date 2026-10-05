use super::*;
use crate::settings::{
    plugins::test_support::{draw, input, menu, shown, shown_settings, tap, wait},
    providers::fixture::Fixture,
};
use sailry_client::Client;
use sailry_protocol::{Command, Output};
#[path = "../../../../../crates/node-runtime/tests/memory_support/mod.rs"]
#[allow(dead_code)]
mod support;
use support::{Entry, Kind, MemoryId, Settings, Summary};

fn execute(fixture: &Fixture, index: usize, command: Command) -> Output {
    let client = Client::new(fixture.transports[index].clone());
    fixture
        .runtime
        .block_on(client.execute(client.prepare(command)))
        .unwrap()
}

fn settings(fixture: &Fixture, index: usize) -> Settings {
    let client = Client::new(fixture.transports[index].clone());
    fixture
        .runtime
        .block_on(support::settings(&client))
        .unwrap()
}

fn entries(fixture: &Fixture, index: usize) -> Vec<Summary> {
    let client = Client::new(fixture.transports[index].clone());
    fixture.runtime.block_on(support::list(&client)).unwrap()
}

fn read(fixture: &Fixture, index: usize, id: MemoryId) -> Entry {
    let client = Client::new(fixture.transports[index].clone());
    fixture
        .runtime
        .block_on(support::read(&client, id))
        .unwrap()
}

fn put(fixture: &Fixture, index: usize, entry: Entry) -> Entry {
    let client = Client::new(fixture.transports[index].clone());
    let revision = entry.summary.revision;
    fixture
        .runtime
        .block_on(support::put(&client, entry, revision))
        .unwrap()
}

fn selector(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[track_caller]
fn ready(owner: &Entity<Workspace>, visual: &mut VisualTestContext, selector: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        draw(visual);
        let snapshot = visual.update(|_, cx| {
            owner
                .read(cx)
                .plugin_settings_panel()
                .map(|panel| crate::plugins::diagnostics(&panel, cx))
                .unwrap_or_else(|| "settings panel missing".into())
        });
        let lines: Vec<_> = snapshot.lines().collect();
        if let Some(index) = lines.iter().position(|line| line.contains(selector)) {
            let indent = |line: &str| line.len() - line.trim_start().len();
            let depth = indent(lines[index]);
            // Registered Kit controls do not include their constructor ID in the
            // snapshot. The test anchor identifies the enclosing control subtree.
            let descendants = lines[index + 1..]
                .iter()
                .take_while(|line| indent(line) > depth);
            let mut ancestor_depth = depth;
            let ancestors = lines[..=index].iter().rev().filter(|line| {
                let candidate = indent(line);
                if candidate < ancestor_depth {
                    ancestor_depth = candidate;
                    true
                } else {
                    false
                }
            });
            if descendants.chain(ancestors).any(|line| {
                (line.trim_start().starts_with("Button") || line.trim_start().starts_with("Switch"))
                    && line.contains(":disabled[Bool(false)]")
            }) {
                return;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "memory control deadline: {selector}; {snapshot}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn install(fixture: &Fixture, index: usize) {
    let root = fixture
        .directory
        .path()
        .join(format!("memory-source-{index}"));
    std::fs::create_dir(&root).unwrap();
    let package = root.join("memory");
    support::prepare(&package);
    // Anchors expose native Kit hit targets without changing callbacks or state.
    let path = package.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&path)
        .unwrap()
        .replace("{Button,Switch,", "{Button as KitButton,Switch,")
        .replace(".child(control));", ".child(Anchor.new(key).child(control)));")
        .replace(
            "new Tab().label(text[`memory_${key}`])",
            "new Tab().child(Anchor.new(`memory-view-${key}`).child(div().child(text[`memory_${key}`])))",
        )
        .replace(
            ".children(summary.revision ? [new Switch('memory-archive')",
            ".children(summary.revision ? [Anchor.new('memory-archive').child(new Switch('memory-archive')",
        )
        .replace(
            ".on_change((value,cx)=>{summary.archived=value;cx.notify();})]",
            ".on_change((value,cx)=>{summary.archived=value;cx.notify();}))]",
        )
        .replace(
            ": merge] : [])",
            ": Anchor.new('memory-merge-source').child(merge)] : [])",
        );
    std::fs::write(
        path,
        format!(
            "import {{Anchor}} from 'sailry/test';\n{source}\nfunction Button(id) {{ const button = new KitButton(id); button.label = text => button.child(Anchor.new(id).child(div().child(text))); return button; }}\n"
        ),
    )
    .unwrap();
    let Output::Project(project) = execute(
        fixture,
        index,
        Command::RegisterProject {
            name: "Memory settings fixture".into(),
            path: root.to_str().unwrap().into(),
        },
    ) else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = execute(fixture, index, Command::Snapshot) else {
        panic!("snapshot expected")
    };
    let worktree = snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.project == Some(project.id))
        .unwrap()
        .id;
    let Output::Plugin(current) = execute(
        fixture,
        index,
        Command::ReadPlugin {
            name: "memory".into(),
        },
    ) else {
        panic!("plugin expected")
    };
    let Output::Plugin(installed) = execute(
        fixture,
        index,
        Command::InstallPlugin {
            worktree,
            path: "memory".into(),
            name: "memory".into(),
            expected_revision: current.summary.revision,
        },
    ) else {
        panic!("plugin expected")
    };
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
}

#[gpui::test]
fn entries_preserve_host_conflicts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    let fixture = Fixture::new();
    for index in 0..2 {
        install(&fixture, index);
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(crate::preferences::Preferences::open(
            fixture.directory.path().join("preferences.json"),
        ));
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
        shell.update(cx, |shell, cx| {
            shell.navigate(crate::preview::Page::Settings, window, cx)
        });
        owner = Some(shell.read(cx).settings.clone());
        Root::new(shell, window, cx)
    });
    let owner = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1100.)));
    let mut prior: Option<WeakEntity<crate::plugins::Panel>> = None;
    for index in 0..2 {
        owner.update(visual, |owner, cx| {
            owner.bind_providers(
                fixture.transports[index].clone(),
                fixture.runtime.clone(),
                format!("Host {index}").into(),
                cx,
            )
        });
        shown(visual, "plugin-settings-memory", true);
        tap(visual, "plugin-settings-memory");
        shown_settings(&owner, visual, "memory-settings", true);
        assert!(visual.debug_bounds("settings-description").is_some());
        owner.read_with(visual, |owner, cx| {
            let description = owner.plugin_description(cx);
            assert!(!description.trim().is_empty());
            assert_ne!(description, tr("plugins_settings_description").to_string());
        });
        wait(visual, |cx| {
            owner.read(cx).plugin_settings_panel().is_some_and(|panel| {
                panel.read(cx).resource_active()
                    && crate::plugins::diagnostics(&panel, cx).contains("memory-create")
            })
        });
        if let Some(prior) = &prior {
            wait(visual, |cx| {
                prior
                    .upgrade()
                    .is_none_or(|panel| !panel.read(cx).resource_active())
            });
        }
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.section, Section::Plugin);
            assert_eq!(owner.selected_plugin_settings(), Some("memory"));
        });
        assert!(entries(&fixture, index).is_empty());
        shown(visual, "memory_auto_write", true);
        ready(&owner, visual, "settings-row-memory_auto_write");
        tap(visual, "memory_auto_write");
        wait(visual, |_| !settings(&fixture, index).auto_write);
        ready(&owner, visual, "settings-row-memory_auto_write");
        tap(visual, "memory_auto_write");
        wait(visual, |_| settings(&fixture, index).auto_write);

        ready(&owner, visual, "memory-create");
        tap(visual, "memory-create");
        shown_settings(&owner, visual, "memory-editor", true);
        input(visual, "field-2", "Preference 中文");
        input(visual, "field-3", "Use concise summaries");
        tap(visual, "memory-save");
        wait(visual, |_| entries(&fixture, index).len() == 1);
        shown_settings(&owner, visual, "memory-editor", false);
        let id = entries(&fixture, index)[0].id;
        assert_eq!(read(&fixture, index, id).body, "Use concise summaries");
        assert_eq!(read(&fixture, index, id).summary.project, None);

        shown(visual, selector(format!("memory-edit-{id}")), true);
        ready(&owner, visual, &format!("memory-edit-{id}"));
        tap(visual, selector(format!("memory-edit-{id}")));
        shown(visual, "field-4", true);
        ready(&owner, visual, "memory-save");
        input(visual, "field-4", "Unsaved local title");
        let mut changed = read(&fixture, index, id);
        changed.summary.title = "Changed elsewhere".into();
        put(&fixture, index, changed);
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "memory-save");
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            "Memory changed or was deleted · reopen to edit"
        );
        shown_settings(&owner, visual, "memory-editor-error", false);
        tap(visual, "field-4");
        visual.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Unsaved local title"
        );
        assert_eq!(read(&fixture, index, id).summary.title, "Changed elsewhere");
        tap(visual, "memory-cancel");
        shown_settings(&owner, visual, "memory-editor", false);

        let source = put(
            &fixture,
            index,
            Entry {
                summary: Summary {
                    id: MemoryId::new(),
                    project: None,
                    title: "Related preference".into(),
                    kind: Kind::User,
                    revision: 0,
                    updated_at_ms: 0,
                    archived: false,
                },
                body: "Include the next action".into(),
            },
        );
        shown_settings(&owner, visual, "Related preference", true);
        shown(visual, selector(format!("memory-edit-{id}")), true);
        ready(&owner, visual, &format!("memory-edit-{id}"));
        tap(visual, selector(format!("memory-edit-{id}")));
        shown(visual, "memory-merge-source", true);
        menu(visual, "memory-merge-source", 0);
        ready(&owner, visual, "memory-save");
        tap(visual, "memory-save");
        wait(visual, |_| {
            read(&fixture, index, source.summary.id).summary.archived
        });
        shown_settings(&owner, visual, "memory-editor", false);
        assert_eq!(
            read(&fixture, index, id).body,
            "Use concise summaries\n\nInclude the next action"
        );
        assert_eq!(read(&fixture, index, id).summary.revision, 3);
        assert_eq!(read(&fixture, index, source.summary.id).summary.revision, 2);

        tap(visual, "memory-view-archived");
        shown(
            visual,
            selector(format!("memory-edit-{}", source.summary.id)),
            true,
        );
        ready(
            &owner,
            visual,
            &format!("memory-edit-{}", source.summary.id),
        );
        tap(
            visual,
            selector(format!("memory-edit-{}", source.summary.id)),
        );
        shown(visual, "memory-archive", true);
        ready(&owner, visual, "memory-archive");
        tap(visual, "memory-archive");
        tap(visual, "memory-save");
        wait(visual, |_| {
            !read(&fixture, index, source.summary.id).summary.archived
        });
        shown_settings(&owner, visual, "memory-editor", false);
        tap(visual, "memory-view-active");
        shown(
            visual,
            selector(format!("memory-delete-{}", source.summary.id)),
            true,
        );
        ready(
            &owner,
            visual,
            &format!("memory-delete-{}", source.summary.id),
        );
        tap(
            visual,
            selector(format!("memory-delete-{}", source.summary.id)),
        );
        shown_settings(&owner, visual, "memory-removal", true);
        tap(visual, "memory-delete-confirm");
        wait(visual, |_| entries(&fixture, index).len() == 1);
        shown_settings(&owner, visual, "memory-removal", false);
        assert_eq!(entries(&fixture, index)[0].id, id);
        prior = Some(owner.read_with(visual, |owner, _| {
            owner.plugin_settings_panel().unwrap().downgrade()
        }));
        draw(visual);
    }
    visual.update(|window, _| window.remove_window());
    drop(owner);
    fixture.close();
}
