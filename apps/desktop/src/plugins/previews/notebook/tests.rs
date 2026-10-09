use super::*;
use crate::plugins::{
    fixture::Fixture,
    tests::{click, init, wait},
};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use sailry_protocol::plugin;
use serde_json::{Value, json};

fn contents() -> String {
    json!({"nbformat":4,"nbformat_minor":5,"metadata":{"kernelspec":{"language":"python"}},"cells":(0..21).map(|index| json!({"cell_type":"code","id":format!("cell-{index}"),"metadata":{},"source":format!("print({index})"),"execution_count":index,"outputs":[{"output_type":"stream","name":"stdout","text":format!("{index}\n")}]})).collect::<Vec<_>>()}).to_string()
}

fn host(fixture: &Fixture) -> Arc<Host> {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "files".into(),
    }) else {
        panic!("Files package expected");
    };
    Host::new(
        fixture.binding.client.clone(),
        plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: info.summary.reference(),
            worktree: fixture.binding.worktree,
            session: Some(fixture.session.id),
        },
        fixture.binding.runtime.handle().clone(),
        true,
        None,
    )
}

fn mount<'a>(
    fixture: &Fixture,
    host: Arc<Host>,
    text: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<View>, &'a mut VisualTestContext) {
    let file = File {
        path: "sample.ipynb".into(),
        name: None,
        mime: "application/x-ipynb+json".into(),
        size: Some(text.len() as u64),
        revision: Some(blake3::hash(text.as_bytes()).to_hex().to_string()),
    };
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| View::new(fixture.binding.clone(), file, host));
        owner = Some(view.clone());
        Root::new(view, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(900.), px(700.)));
    (owner.unwrap(), visual)
}

#[gpui_kit::test]
fn pages_captured_outputs_without_executing_cells(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let path = fixture.directory.path().join("project/sample.ipynb");
        let text = contents();
        std::fs::write(&path, &text).unwrap();
        let host = host(&fixture);
        let (view, visual) = mount(&fixture, host.clone(), &text, cx);
        wait(visual, |cx| view.read(cx).notebook.is_some());
        assert_eq!(
            view.read_with(visual, |view, _| view.notebook.as_ref().unwrap().count()),
            20
        );
        click(visual, "notebook-resource-more");
        wait(visual, |cx| {
            view.read(cx).notebook.as_ref().unwrap().next.is_none()
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.notebook.as_ref().unwrap().count()),
            21
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .notebook
                .as_ref()
                .unwrap()
                .source(20, cx)),
            "print(20)"
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
        let requests: Vec<_> = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::ReadNotebook { .. }))
            .cloned()
            .collect();
        assert_eq!(requests.len(), 2);
        for (offset, request) in [0, 20].into_iter().zip(requests) {
            assert_eq!(request.target, fixture.node.id());
            assert_eq!(request.plugin.as_ref(), Some(&host.context()));
            assert!(
                matches!(request.command, Command::ReadNotebook { worktree, options } if Some(worktree) == fixture.binding.worktree && options.path == "sample.ipynb" && options.offset == offset)
            );
        }
        host.close();
        visual.update(|window, _| window.remove_window());
        drop(view);
        drop(host);
        fixture.close();
    }
}

#[gpui_kit::test]
fn retains_cells_when_captured_revision_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let path = fixture.directory.path().join("project/sample.ipynb");
        let text = contents();
        std::fs::write(&path, &text).unwrap();
        let host = host(&fixture);
        let (view, visual) = mount(&fixture, host.clone(), &text, cx);
        wait(visual, |cx| view.read(cx).notebook.is_some());
        let mut changed: Value = serde_json::from_str(&text).unwrap();
        changed["cells"][20]["source"] = json!("External change");
        std::fs::write(&path, changed.to_string()).unwrap();
        click(visual, "notebook-resource-more");
        wait(visual, |cx| view.read(cx).failed && !view.read(cx).loading);
        assert_eq!(
            view.read_with(visual, |view, _| view.notebook.as_ref().unwrap().count()),
            20
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.notebook.as_ref().unwrap().next),
            Some(20)
        );
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            "notebook changed while reading cells"
        );
        host.close();
        visual.update(|window, _| window.remove_window());
        drop(view);
        drop(host);
        fixture.close();
    }
}
