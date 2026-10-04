use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn feedback_and_names(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut shell = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Shell::new(window, cx));
        shell = Some(view.clone());
        Root::new(view, window, cx)
    });
    let shell = shell.unwrap();
    let editor = visual.update(|window, cx| {
        shell
            .update(cx, |shell, cx| shell.project_editor(0, None, window, cx))
            .unwrap()
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let body = visual.debug_bounds("project-editor").unwrap();
    let save = visual.debug_bounds("project-save").unwrap();
    visual.simulate_click(save.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
        assert_eq!(editor.read(cx).error, Some("project_required"));
    });
    assert_eq!(
        visual.debug_bounds("project-editor").unwrap().size,
        body.size
    );
    assert_eq!(
        visual.debug_bounds("project-save").unwrap().top()
            - visual.debug_bounds("project-editor").unwrap().top(),
        save.top() - body.top()
    );
    visual.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.pending = true;
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(
        visual.debug_bounds("project-editor").unwrap().size,
        body.size
    );
    assert_eq!(
        visual.debug_bounds("project-save").unwrap().top()
            - visual.debug_bounds("project-editor").unwrap().top(),
        save.top() - body.top()
    );

    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.pending = false;
            for (path, expected) in [
                ("/Volumes/Data/test2", "test2"),
                ("/workspace/project/", "project"),
                (r"C:\workspace\project\", "project"),
                ("/workspace/测试项目/", "测试项目"),
            ] {
                editor.inputs[0].update(cx, |input, cx| input.set_value("  ", window, cx));
                editor.inputs[1].update(cx, |input, cx| input.set_value(path, window, cx));
                editor.suggest(window, cx);
                assert_eq!(editor.inputs[0].read(cx).value(), expected);
            }
            editor.inputs[0].update(cx, |input, cx| input.set_value("Custom", window, cx));
            editor.suggest(window, cx);
            assert_eq!(editor.inputs[0].read(cx).value(), "Custom");
            // Saving after clearing the suggestion restores only the directory name.
            editor.inputs[0].update(cx, |input, cx| input.set_value("", window, cx));
            editor.inputs[1].update(cx, |input, cx| {
                input.set_value("/preview/test2", window, cx)
            });
            editor.save(window, cx);
        });
        assert!(
            shell
                .read(cx)
                .workspace
                .projects
                .values()
                .any(|project| { project.path == "/preview/test2" && project.name == "test2" })
        );
    });
}
