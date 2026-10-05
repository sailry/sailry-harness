use super::*;
use gpui_kit::component::input::{EditorState, Position};

fn position(
    visual: &mut VisualTestContext,
    panel: &Entity<Panel>,
    input: &Entity<EditorState>,
    id: &str,
    line: u32,
    column: u32,
) {
    let label = crate::tr("files_cursor_position")
        .replace("{line}", &line.to_string())
        .replace("{column}", &column.to_string());
    let selector: &'static str =
        Box::leak(format!("document-text-document-cursor-position-{label}").into_boxed_str());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (ready, diagnostics) = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let document = value(panel, id, cx);
            let rendered = snapshot(panel, cx);
            let editor = input.read(cx);
            (
                document["position"] == serde_json::json!({"line":line,"column":column})
                    && document["language"] == editor.language_name().as_ref(),
                format!(
                    "native={:?}, selection={:?}, focused={}, document={document}, label={label:?}, rendered={rendered}",
                    editor.cursor_position(),
                    editor.selected_range(),
                    editor.focus_handle(cx).is_focused(window),
                ),
            )
        });
        if ready && visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "status deadline: {diagnostics}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn shows_native_cursor_and_language(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        std::fs::write(
            fixture.directory.path().join("project/sample.rs"),
            "first\né🚢x\n",
        )
        .unwrap();
        std::fs::write(
            fixture.directory.path().join("project/sample.md"),
            "**é🚢x**\n",
        )
        .unwrap();
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        click(visual, "resource-file-sample.rs");
        let id = document(&panel, visual, "sample.rs");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let input = controller.read_with(visual, |controller, _| {
            controller.editor("sample.rs").unwrap()
        });
        assert_eq!(
            input.read_with(visual, |input, _| input.language_name()),
            "rs"
        );
        visual.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.set_cursor_position(Position::new(1, 2), window, cx);
            });
        });
        for column in [3, 2] {
            position(visual, &panel, &input, &id, 2, column);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let path = visual.debug_bounds("document-path-label").unwrap();
            let position = visual.debug_bounds("document-cursor-position").unwrap();
            let encoding = visual.debug_bounds("document-encoding").unwrap();
            let language = visual.debug_bounds("document-language").unwrap();
            assert!(path.right() <= position.left());
            assert!(position.right() < encoding.left());
            assert!(encoding.right() < language.left());
            assert!(visual.debug_bounds("document-diff-count").is_none());
            assert!(
                visual
                    .debug_bounds("document-text-document-encoding-UTF-8")
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds("document-text-document-language-rs")
                    .is_some()
            );
            if column == 3 {
                visual.simulate_keystrokes("left");
            }
        }
        click(visual, "resource-file-sample.md");
        let markdown = document(&panel, visual, "sample.md");
        wait(visual, |cx| {
            value(&panel, &markdown, cx)["mode"] == "document"
        });
        let source = controller.read_with(visual, |controller, _| {
            controller.editor("sample.md").unwrap()
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let paragraph = visual.debug_bounds("markdown-editor-block-0").unwrap();
        visual.simulate_click(paragraph.center(), Modifiers::default());
        visual.simulate_keystrokes("home right right");
        for column in [5, 4] {
            position(visual, &panel, &source, &markdown, 1, column);
            assert_eq!(
                visual.update(|_, cx| value(&panel, &markdown, cx)["dirty"].clone()),
                false
            );
            assert_eq!(
                source.read_with(visual, |source, _| source.cursor_position()),
                Position::new(0, 0)
            );
            if column == 5 {
                visual.simulate_keystrokes("left");
            }
        }
        visual.update(|window, _| window.remove_window());
        drop(source);
        drop(input);
        drop(controller);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
