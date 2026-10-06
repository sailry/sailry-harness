use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::ModelApi;

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn enter(draft: &Draft, row: usize, value: &str, window: &mut Window, cx: &mut App) {
    draft.efforts.entries[row]
        .input
        .update(cx, |input, cx| input.set_value(value, window, cx));
}

fn valid(draft: &Draft, api: ModelApi, cx: &App) -> bool {
    draft.value(cx).is_ok_and(|model| {
        super::super::super::live::model(&model)
            .validate_reasoning(api)
            .is_ok()
    })
}

#[gpui::test]
fn preserves_raw_values_in_chinese(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    let before = rust_i18n::locale().to_string();
    rust_i18n::set_locale("zh-CN");
    visual.update(|window, cx| {
        let mut model = Model::example("fixture");
        model.efforts = vec![
            Effort::Disabled,
            Effort::Minimal,
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::XHigh,
            Effort::Max,
            Effort::Budget(-1),
            Effort::Budget(1024),
        ];
        model.default_effort = Effort::High;
        let draft = Draft::new(0, model.clone(), window, cx);
        let values: Vec<_> = draft
            .efforts
            .entries
            .iter()
            .map(|row| row.input.read(cx).value().to_string())
            .collect();
        assert_eq!(
            values,
            [
                "none", "minimal", "low", "medium", "high", "xhigh", "max", "-1", "1024"
            ]
        );
        let saved = draft.value(cx);
        rust_i18n::set_locale(&before);
        assert_eq!(saved.unwrap(), model);
    });
}

#[gpui::test]
fn edits_and_defaults(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = Draft::new(0, Model::example("fixture"), window, cx);
        assert!(!draft.value(cx).unwrap().custom_efforts);
        enter(&draft, 0, "minimal", window, cx);
        draft.model.reasoning = false;
        assert_eq!(draft.value(cx).unwrap().efforts[0], Effort::Minimal);
        draft.model.reasoning = true;
        enter(&draft, 0, "low", window, cx);
        draft.efforts.edit(Edit::Add, window, cx);
        assert!(draft.efforts.entries[3].input.read(cx).value().is_empty());
        assert!(draft.value(cx).is_err());
        enter(&draft, 3, "xhigh", window, cx);
        draft.efforts.edit(Edit::Default(3), window, cx);
        assert_eq!(draft.value(cx).unwrap().default_effort, Effort::XHigh);
        enter(&draft, 3, "max", window, cx);
        assert_eq!(draft.value(cx).unwrap().default_effort, Effort::Max);
        draft.efforts.edit(Edit::Remove(3), window, cx);
        assert_eq!(draft.value(cx).unwrap().default_effort, Effort::Low);
        for key in [1, 2, 0] {
            draft.efforts.edit(Edit::Remove(key), window, cx);
        }
        assert!(draft.value(cx).unwrap().efforts.is_empty());
        assert_eq!(draft.value(cx).unwrap().default_effort, Effort::Default);
        assert!(valid(&draft, ModelApi::Responses, cx));
        draft.efforts.edit(Edit::Add, window, cx);
        assert_eq!(draft.efforts.entries[0].key, 4);
        enter(&draft, 0, "high", window, cx);
        assert_eq!(draft.value(cx).unwrap().default_effort, Effort::High);
        draft.efforts.edit(Edit::Remove(4), window, cx);
        let saved = draft.value(cx).unwrap();
        assert!(saved.custom_efforts);
        assert_eq!(
            Draft::new(0, saved.clone(), window, cx).value(cx).unwrap(),
            saved
        );
    });
}

#[gpui::test]
fn validates_inputs(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = Draft::new(0, Model::example("fixture"), window, cx);
        for value in ["", "default", "unknown", "medium"] {
            enter(&draft, 0, value, window, cx);
            assert!(!valid(&draft, ModelApi::Responses, cx));
        }
        enter(&draft, 0, "1024", window, cx);
        assert!(!valid(&draft, ModelApi::Responses, cx));
        assert!(valid(&draft, ModelApi::Anthropic, cx));
        for value in ["512", "16384"] {
            enter(&draft, 0, value, window, cx);
            assert!(!valid(&draft, ModelApi::Anthropic, cx));
        }
        enter(&draft, 0, "-1", window, cx);
        assert!(valid(&draft, ModelApi::Gemini, cx));
        for tokens in 1024..1037 {
            draft.efforts.edit(Edit::Add, window, cx);
            enter(
                &draft,
                draft.efforts.entries.len() - 1,
                &tokens.to_string(),
                window,
                cx,
            );
        }
        draft.efforts.edit(Edit::Add, window, cx);
        assert_eq!(draft.efforts.entries.len(), 16);
        assert!(valid(&draft, ModelApi::Gemini, cx));
    });
}

#[gpui::test]
fn preserves_custom_refresh(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = Draft::new(0, Model::example("fixture"), window, cx);
        enter(&draft, 0, "max", window, cx);
        assert!(draft.value(cx).unwrap().custom_efforts);
        for key in [1, 2] {
            draft.efforts.edit(Edit::Remove(key), window, cx);
        }
        let saved = draft.value(cx).unwrap();
        let mut reopened = Draft::new(0, saved.clone(), window, cx);
        let native = sailry_protocol::conversation::discovery::Model {
            id: "fixture".into(),
            context: None,
            output: None,
            capabilities: Some(sailry_protocol::conversation::discovery::Capabilities {
                efforts: Some(vec![Effort::Low, Effort::High]),
                default_effort: Some(Effort::High),
                ..Default::default()
            }),
        };
        reopened.fill(
            Some(&native),
            None,
            super::super::super::Preset::OpenAi,
            window,
            cx,
        );
        assert_eq!(reopened.value(cx).unwrap(), saved);
        draft.efforts.edit(Edit::Add, window, cx);
        draft.fill(
            Some(&native),
            None,
            super::super::super::Preset::OpenAi,
            window,
            cx,
        );
        assert!(draft.value(cx).is_err());
        assert_eq!(draft.efforts.entries.len(), 2);
        assert!(draft.efforts.entries[1].input.read(cx).value().is_empty());
    });
}
