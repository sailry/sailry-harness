use super::*;
use crate::{
    preview::Page,
    resources::SideResource,
    shell::{Shell, session_scope::Key},
};
use sailry_protocol::{Output, plugin::Info};

mod closing;
mod editing;
mod references;
mod status;
mod watching;
mod workspace;

fn package(fixture: &Fixture) -> Info {
    let Output::Plugin(package) = fixture.execute(Command::ReadPlugin {
        name: "files".into(),
    }) else {
        panic!("Files package expected");
    };
    package
}

fn enable(fixture: &Fixture, enabled: bool) {
    let package = package(fixture);
    fixture.execute(Command::SetPluginEnabled {
        name: package.summary.name,
        expected_revision: package.summary.revision,
        enabled,
    });
}

fn install(fixture: &Fixture) {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/files");
    let destination = fixture.directory.path().join("project/files-package");
    super::super::fixture::copy_package(&source, &destination);
    let view = destination.join("dev.sailry.platform/desktop/explorer-view.js");
    let source = std::fs::read_to_string(&view).unwrap();
    let button =
        "new Button(`file-search-result-${index}`).ghost().w_full().h('auto').justify_start()";
    assert!(source.contains(button), "Search result button expected");
    // Script Button IDs are not debug selectors. An absolute child measures the
    // existing native button without changing its content, layout, or callback.
    let source = source.replacen(
        button,
        &format!("{button}.child(Bounds.new(`file-search-result-${{index}}`))"),
        1,
    );
    std::fs::write(
        &view,
        format!("import {{Bounds}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    let dialogs = destination.join("dev.sailry.platform/desktop/dialogs.js");
    let mut source = std::fs::read_to_string(&dialogs).unwrap();
    for id in ["file-transfer-close", "file-transfer-confirm"] {
        let button = format!("new Button('{id}')");
        assert!(source.contains(&button), "Transfer button expected");
        source = source.replacen(&button, &format!("{button}.child(Bounds.new('{id}'))"), 1);
    }
    std::fs::write(
        &dialogs,
        format!("import {{Bounds}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    let document = destination.join("dev.sailry.platform/desktop/document-view.js");
    let mut source = std::fs::read_to_string(&document).unwrap();
    let path =
        "div().id('document-path-label').flex_1().min_w_0().truncate().child(document?.path ?? '')";
    assert!(source.contains(path), "Document path label expected");
    source = source.replacen(
        path,
        &format!("{path}.relative().child(Bounds.new('document-path-label'))"),
        1,
    );
    // Native named slots are materialized after ScriptView's shallow snapshot.
    // Measure each original text label without changing its layout or content.
    for (id, label) in [
        (
            "document-cursor-position",
            "text.files_cursor_position\n      .replace('{line}',String(document.position.line)).replace('{column}',String(document.position.column))",
        ),
        ("document-encoding", "'UTF-8'"),
        ("document-language", "document.language"),
    ] {
        let original = format!("div().id('{id}').child({label})");
        assert!(source.contains(&original), "Document status label expected");
        source = source.replacen(&original, &format!("statusLabel('{id}',{label})"), 1);
    }
    std::fs::write(
        &document,
        format!(
            "import {{Bounds}} from 'sailry/test';\n{source}\nfunction statusLabel(id,label) {{return div().id(id).relative().child(label).child(Bounds.new(id)).child(Bounds.new(`document-text-${{id}}-${{label}}`));}}\n"
        ),
    )
    .unwrap();
    let Output::Plugin(installed) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "files-package".into(),
        name: "files".into(),
        expected_revision: package(fixture).summary.revision,
    }) else {
        panic!("Files package expected");
    };
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    if !installed.summary.enabled {
        enable(fixture, true);
    }
}

pub(super) fn mount<'a>(
    fixture: &Fixture,
    remote: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        let mut services = fixture.services(remote);
        if !remote {
            services.local = fixture.transport.clone();
        }
        cx.set_global(services);
        cx.set_global(crate::preferences::Preferences::default());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(window, cx);
            shell
                .live
                .as_mut()
                .unwrap()
                .override_transport(fixture.transport.clone());
            shell
        });
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .transport_for(fixture.node.id())
            .is_some()
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.node == fixture.node.id()
                    && snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == fixture.session.id)
            })
    });
    (shell, visual)
}

fn main(shell: &Entity<Shell>, fixture: &Fixture, visual: &mut VisualTestContext) -> Entity<Panel> {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live_resource_action(
                &crate::live::menus::Dispatch {
                    node: fixture.node.id(),
                    target: crate::live::menus::Target::Project(fixture.binding.project.unwrap()),
                    command: crate::live::menus::Command::Open,
                },
                window,
                cx,
            );
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|entry| entry.package.name == "files")
    });
    let selector = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.package.name == "files")
            .unwrap()
            .selector()
    });
    click(visual, Box::leak(selector.into_boxed_str()));
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    ready(&panel, visual);
    assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
    panel
}

#[track_caller]
fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    // Published names can appear in tabs before the filesystem tree refreshes.
    // Wait for the actual control, then dispatch the same single click.
    let bounds = loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if let Some(bounds) = visual.debug_bounds(selector) {
            break bounds;
        }
        assert!(Instant::now() < deadline, "missing {selector}");
        std::thread::sleep(Duration::from_millis(10));
    };
    visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

fn ready(panel: &Entity<Panel>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        let state = panel.read(cx);
        let rendered = snapshot(panel, cx);
        state.resource_active()
            && state.documents.is_some()
            && rendered.contains("file-explorer")
            && rendered.contains("notes.txt")
            && rendered.lines().any(|line| {
                line.contains("Menu \"file-create-menu\"")
                    && line.contains("(\"disabled\", Bool(false))")
            })
    });
}

#[track_caller]
fn wait_for(panel: &Entity<Panel>, visual: &mut VisualTestContext, selector: &str) {
    let deadline = Instant::now() + Duration::from_secs(8);
    wait(visual, |cx| {
        let state = snapshot(panel, cx);
        if state.contains(selector) {
            return true;
        }
        assert!(
            Instant::now() < deadline,
            "missing {selector}; documents={}; rendered={state}",
            documents(panel, cx)
        );
        false
    });
}

fn documents(panel: &Entity<Panel>, cx: &App) -> serde_json::Value {
    panel
        .read(cx)
        .documents
        .as_ref()
        .unwrap()
        .read(cx)
        .snapshot(cx)
}

fn document(panel: &Entity<Panel>, visual: &mut VisualTestContext, path: &str) -> String {
    wait(visual, |cx| {
        documents(panel, cx)["documents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|document| document["path"] == path)
    });
    visual.update(|_, cx| {
        documents(panel, cx)["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|document| document["path"] == path)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    })
}

fn value(panel: &Entity<Panel>, id: &str, cx: &App) -> serde_json::Value {
    documents(panel, cx)["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|document| document["id"] == id)
        .unwrap()
        .clone()
}

fn edit(panel: &Entity<Panel>, visual: &mut VisualTestContext, id: &str, text: &str) {
    click(
        visual,
        Box::leak(format!("file-editor-{id}").into_boxed_str()),
    );
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_input(text);
    wait(visual, |cx| value(panel, id, cx)["dirty"] == true);
}
