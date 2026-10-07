use super::*;
use crate::plugins::tests::{click, init, wait};
use core::prelude::v1::test;
use gpui_shell::{
    HostValue,
    policy::{self, Policy},
};

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

#[test]
fn validates_nested_items() {
    for items in [
        "{]",
        "{}",
        r#"[{"id":"leaf","label":"Leaf","unknown":true}]"#,
        r#"[{"id":"folder","label":"Folder","children":[{"id":"leaf","label":"Leaf","unknown":true}]}]"#,
        r#"[{"id":"leaf","label":"Leaf","menu":[{"label":"Open","enabled":"true"}]}]"#,
        r#"[{"id":"leaf","label":"Leaf","decoration":{"check":{"id":"check","label":"Stage","checked":"false"}}}]"#,
    ] {
        assert!(serde_json::from_value::<Props>(json!({"items":items})).is_err());
    }
}

#[gpui::test]
fn preserves_deep_tree_interactions(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        include_str!("tests/view.js"),
    )
    .unwrap();
    let events = Rc::new(RefCell::new(Vec::<Value>::new()));
    let records = events.clone();
    let probes = HostModule::new("sailry/test")
        .component("Anchor", |mut args, _, _| {
            div()
                .debug_selector(|| args.id().to_owned())
                .children(args.take_children())
                .into_any_element()
        })
        .function("record", move |args| {
            records.borrow_mut().push(
                serde_json::from_str(args.string(0)?)
                    .map_err(|error| HostError::new(error.to_string()))?,
            );
            Ok(HostValue::Null)
        });
    let stop = CancellationToken::new();
    let ui = cx.update(|cx| {
        module(
            native_context::module(HostModule::new("sailry/ui"), stop.clone(), cx),
            stop.clone(),
            drops::Drops::new(stop.clone()),
            cx,
        )
    });
    let granted = Policy::new()
        .with_capabilities(
            gpui_shell::Capabilities::new().read_roots([directory.path().to_path_buf()]),
        )
        .with_host_module(ui)
        .unwrap()
        .with_host_module(probes)
        .unwrap();
    let mut previous = None;
    policy::update_default(|current| {
        previous = Some(current);
        granted
    });
    let _restore = DefaultPolicy(previous);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let mut application = None;
    let mut script = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = runtime.try_load(directory.path(), window, cx).unwrap();
        let content = view
            .read(cx)
            .content()
            .clone()
            .downcast::<gpui_shell::ScriptView>()
            .unwrap();
        script = Some(content.clone());
        application = Some(view);
        Root::new(content, window, cx)
    });
    let _application = application.unwrap();
    let script = script.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1000.), px(900.)));
    wait(visual, |cx| {
        assert_eq!(script.read(cx).build_error(), None);
        true
    });
    let leaf = visual.debug_bounds("resource-file-leaf").unwrap();
    let tree = visual.debug_bounds("deep-tree").unwrap();
    assert!(tree.contains(&leaf.center()));
    click(visual, "deep-check");
    wait(visual, |_| events.borrow().len() == 1);
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"leaf","kind":"check"})
    );
    click(visual, "resource-label-leaf");
    wait(visual, |_| events.borrow().len() == 2);
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"leaf","kind":"select","shift":false,"additive":false})
    );
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"leaf","kind":"open"})
    );
    click(visual, "refresh");
    wait(visual, |_| true);
    visual.simulate_keystrokes("enter");
    wait(visual, |_| events.borrow().len() == 1);
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"leaf","kind":"open"})
    );
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let leaf = visual.debug_bounds("resource-file-leaf").unwrap();
        visual.simulate_mouse_down(leaf.center(), MouseButton::Right, Modifiers::default());
        wait(visual, |_| events.borrow().len() == 1);
        assert_eq!(
            events.borrow_mut().remove(0),
            json!({"tree":"deep-tree","id":"leaf","kind":"context"})
        );
        // Dispatch the actual captured native descriptor; TestWindow cannot open an OS popup.
        visual.update(|window, cx| native_context::choose("deep-tree", "Open", window, cx));
        wait(visual, |_| events.borrow().len() == 1);
        assert_eq!(
            events.borrow_mut().remove(0),
            json!({"tree":"deep-tree","id":"leaf","kind":"menu","action":"open",
                "value":{"resource":{"node":"fixture","worktree":"tree","entry":{"path":"长目录/leaf.rs","revision":1}},"selection":["leaf"],"metadata":{"source":"deep"}}})
        );
        visual.simulate_mouse_up(leaf.center(), MouseButton::Right, Modifiers::default());
    }
    click(visual, "resource-file-folder-0");
    wait(visual, |_| events.borrow().len() == 2);
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"folder-0","kind":"select","shift":false,"additive":false})
    );
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"folder-0","kind":"collapse"})
    );
    assert!(visual.debug_bounds("resource-file-leaf").is_none());
    click(visual, "refresh");
    wait(visual, |_| true);
    assert!(visual.debug_bounds("resource-file-leaf").is_none());
    assert!(events.borrow().is_empty());
    click(visual, "resource-file-folder-0");
    wait(visual, |_| events.borrow().len() == 2);
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"folder-0","kind":"select","shift":false,"additive":false})
    );
    assert_eq!(
        events.borrow_mut().remove(0),
        json!({"tree":"deep-tree","id":"folder-0","kind":"expand"})
    );
    assert!(visual.debug_bounds("resource-file-leaf").is_some());
    visual.update(|_, cx| assert_eq!(script.read(cx).build_error(), None));
    stop.cancel();
    visual.update(|window, _| window.remove_window());
}
