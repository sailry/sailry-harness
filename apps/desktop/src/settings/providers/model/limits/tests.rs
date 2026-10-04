use super::*;
use crate::settings::Workspace;
use core::prelude::v1::test;

struct ModelForm(Entity<Editor>);

impl Render for ModelForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .child(self.0.update(cx, |editor, cx| editor.render_model(0, cx)))
    }
}

#[gpui::test]
fn presets_preserve_model_output(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut editor = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let state = cx.new(|cx| Editor::new(workspace, Some(0), window, cx));
        state.update(cx, |editor, _| {
            editor.models[0].expanded = true;
            editor.models[0].advanced = false;
        });
        editor = Some(state.clone());
        let view = cx.new(|_| ModelForm(state));
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();
    let (key, output) = editor.read_with(visual, |editor, cx| {
        (
            editor.models[0].key,
            editor.models[0].value(cx).unwrap().output,
        )
    });
    let context = Box::leak(format!("model-context-input-{key}").into_boxed_str());
    let output_input = Box::leak(format!("model-output-input-{key}").into_boxed_str());
    let advanced = Box::leak(format!("model-advanced-{key}").into_boxed_str());
    for width in [640., 400.] {
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(width), px(700.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual.debug_bounds(context).unwrap();
        assert!(bounds.right() <= px(width));
        assert!(visual.debug_bounds(output_input).is_none());
        assert!(bounds.bottom() <= visual.debug_bounds(advanced).unwrap().top());
        for tokens in PRESETS {
            let preset = Box::leak(format!("model-context-{key}-{tokens}").into_boxed_str());
            let button = visual.debug_bounds(preset).unwrap();
            assert!(button.right() <= bounds.right() + px(1.));
            assert!(button.bottom() <= bounds.top());
            visual.simulate_click(button.center(), Modifiers::default());
            visual.run_until_parked();
            editor.read_with(visual, |editor, cx| {
                assert_eq!(
                    editor.models[0].context.read(cx).value(),
                    tokens.to_string()
                );
                assert_eq!(editor.models[0].value(cx).unwrap().output, output);
                assert!(!editor.models[0].missing.context);
                assert!(!editor.models[0].advanced);
                assert_eq!(editor.models[0].value(cx).unwrap().context, tokens);
            });
        }
    }
    editor.update(visual, |editor, cx| {
        editor.pending = true;
        cx.notify();
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let button = visual
        .debug_bounds(Box::leak(
            format!("model-context-{key}-128000").into_boxed_str(),
        ))
        .unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    visual.run_until_parked();
    assert_eq!(
        editor.read_with(visual, |editor, cx| editor.models[0]
            .context
            .read(cx)
            .value()),
        "1000000"
    );
}
