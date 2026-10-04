use super::*;
use crate::settings::providers::Preset;
use core::prelude::v1::test;
use sailry_protocol::conversation::{catalog, discovery};

struct View;
impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn default_capabilities(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        for (context, output, expected) in [
            (None, None, (200_000, 16_384)),
            (Some(128_000), None, (128_000, 16_384)),
            (Some(1_000_000), None, (1_000_000, 16_384)),
            (Some(8192), None, (8192, 8192)),
            (None, Some(512_000), (512_000, 512_000)),
            (Some(2048), Some(128), (2048, 128)),
        ] {
            let draft = Draft::discovered(
                0,
                discovery::Model {
                    id: "fixture".into(),
                    context,
                    output,
                    capabilities: None,
                },
                None,
                Preset::CompatibleResponses,
                window,
                cx,
            );
            let model = draft.value(cx).unwrap();
            assert_eq!((model.context, model.output), expected);
            assert!(!model.vision && !model.tools && !model.reasoning && !model.web_search);
            assert!(model.generates.is_empty());
        }
        let draft = Draft::discovered(
            0,
            discovery::Model {
                id: "fixture".into(),
                context: Some(2048),
                output: Some(4096),
                capabilities: None,
            },
            None,
            Preset::CompatibleResponses,
            window,
            cx,
        );
        assert_eq!(draft.value(cx).unwrap_err(), "provider_limits_invalid");
    });
}

#[gpui::test]
fn explicit_metadata_precedence(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let reference = catalog::Model {
            id: "fixture".into(),
            name: "Fixture".into(),
            context: Some(8192),
            output: Some(512),
            inputs: vec!["text".into(), "image".into()],
            outputs: vec!["text".into()],
            tools: Some(true),
            reasoning: Some(true),
            options: vec![],
        };
        let discovered = discovery::Model {
            id: "fixture".into(),
            context: Some(4096),
            output: None,
            capabilities: None,
        };
        let model = Draft::discovered(
            0,
            discovered.clone(),
            Some(&reference),
            Preset::CompatibleChat,
            window,
            cx,
        )
        .value(cx)
        .unwrap();
        assert_eq!((model.context, model.output), (4096, 512));
        assert!(model.vision && model.tools && model.reasoning);
        assert!(model.efforts.is_empty());
        let mut discovered = discovered;
        discovered.capabilities = Some(discovery::Capabilities {
            vision: Some(false),
            tools: Some(false),
            reasoning: Some(false),
            ..Default::default()
        });
        let draft = Draft::discovered(
            0,
            discovered,
            Some(&reference),
            Preset::CompatibleChat,
            window,
            cx,
        );
        let model = draft.value(cx).unwrap();
        assert!(!model.vision && !model.tools && !model.reasoning);
        draft
            .context
            .update(cx, |input, cx| input.set_value("-1", window, cx));
        assert_eq!(draft.value(cx).unwrap_err(), "provider_limits_invalid");
    });
}
