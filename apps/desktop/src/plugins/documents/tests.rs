use super::*;
use crate::plugins::{
    Panel,
    fixture::Fixture,
    tests::{click, init, snapshot, wait},
};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use sailry_protocol::{Command, Output};
mod closing;
mod observed;
mod publication;

#[gpui_kit::test]
fn retains_buffers_and_applies_confirmed_entries(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/dev.sailry.platform/desktop/main.js"),
            include_str!("tests/view.js"),
        )
        .unwrap();
        let package = fixture.install(0);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let documents = Store::default()
                .get(fixture.binding.clone(), None, window, cx)
                .unwrap();
            let panel = cx.new(|cx| {
                Panel::standalone(fixture.binding.clone(), cx).with_documents(Some(documents))
            });
            owner = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let panel = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1000.), px(700.)));
        wait(visual, |cx| {
            panel.read(cx).ready_for(&package.summary.reference(), cx)
        });
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("document-status-ready")
        });
        click(visual, "resource-file-notes.txt");
        wait(visual, |cx| snapshot(&panel, cx).contains("document-clean"));
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let (id, input) = controller.read_with(visual, |controller, _| {
            controller
                .documents
                .iter()
                .next()
                .map(|(id, document)| (*id, document.input.clone()))
                .unwrap()
        });
        click(visual, "document-focus");
        wait(visual, |_| true);
        assert!(visual.update(|window, cx| input.focus_handle(cx).is_focused(window)));
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("Retained draft\nSecond line");
        wait(visual, |cx| controller.read(cx).documents[&id].dirty(cx));
        click(visual, "document-rename");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("document-status-blocked")
        });
        assert!(fixture.directory.path().join("project/notes.txt").is_file());
        assert!(
            !fixture
                .directory
                .path()
                .join("project/renamed.txt")
                .exists()
        );
        let Output::Plugin(disabled) = fixture.execute(Command::SetPluginEnabled {
            name: package.summary.name.clone(),
            expected_revision: package.summary.revision,
            enabled: false,
        }) else {
            panic!("plugin expected")
        };
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "Retained draft\nSecond line"
        );
        let Output::Plugin(enabled) = fixture.execute(Command::SetPluginEnabled {
            name: disabled.summary.name,
            expected_revision: disabled.summary.revision,
            enabled: true,
        }) else {
            panic!("plugin expected")
        };
        let expected = enabled.summary.reference();
        wait(visual, |cx| {
            let state = panel.read(cx);
            assert!(
                matches!(state.error, None | Some("plugins_view_unavailable")),
                "plugin {:?}: {:?}",
                state.selected,
                state.error
            );
            state.available(&expected)
                && state.selected.as_ref() == Some(&expected)
                && state.mounted.is_some()
                && state.error.is_none()
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("document-dirty"));
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.documents[&id]
                .input
                .entity_id()),
            input.entity_id()
        );
        click(visual, "document-focus");
        visual.run_until_parked();
        visual.simulate_keystrokes("secondary-s");
        wait(visual, |cx| {
            !controller.read(cx).documents[&id].dirty(cx)
                && !controller.read(cx).documents[&id].saving
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/notes.txt")).unwrap(),
            "Retained draft\nSecond line"
        );
        click(visual, "document-rename");
        wait(visual, |cx| {
            controller.read(cx).documents[&id].path == "renamed.txt"
        });
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.documents[&id]
                .input
                .entity_id()),
            input.entity_id()
        );
        assert!(
            fixture
                .directory
                .path()
                .join("project/renamed.txt")
                .is_file()
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}
