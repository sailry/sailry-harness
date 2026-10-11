use super::*;
use core::prelude::v1::test;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::{Theme, ThemeMode};
use sailry_protocol::{SessionId, TurnId, artifact::ViewContent, plugin};

fn content() -> Content {
    Content {
        artifact: Summary {
            id: ArtifactId::new(),
            session: SessionId::new(),
            turn: TurnId::new(),
            package: plugin::Reference {
                name: "document-fixture".into(),
                digest: "frozen".into(),
                settings_revision: 0,
            },
            title: "Architecture".into(),
            summary: "Captured resources".into(),
            revision: 1,
            size: 20,
            created_at: 1,
            updated_at: 1,
        },
        latest_revision: 1,
        html: "<h1>Architecture</h1>".into(),
        warnings: vec![],
        view: Some(ViewContent {
            template:
                "<!doctype html><html><head></head><body><main id=\"brief\"></main></body></html>"
                    .into(),
            styles: vec!["main{color:var(--document-foreground)}".into()],
            scripts: vec!["window.sailryDocument={mount:input=>{window.fixture=input}};".into()],
            libraries: vec![],
        }),
    }
}

#[test]
fn confines_content_and_preserves_capture() {
    let mut content = content();
    content.html = "<p>Closing </script> content 中文 🙂</p>".into();
    let page = document(
        &content,
        json!({"primary":"#fff"}),
        json!({"submit":"Send"}),
        false,
        vec![],
    )
    .unwrap();
    assert!(page.contains("connect-src 'none'"));
    assert!(page.contains("form-action 'none'"));
    assert!(page.contains("frame-src 'none'"));
    assert!(page.contains(&content.artifact.id.to_string()));
    assert!(page.contains(&content.artifact.session.to_string()));
    assert!(page.contains("\"revision\":1"));
    assert!(page.contains("\"writable\":false"));
    assert!(page.contains("\\u003c/script>"));
    assert_eq!(page.matches("<script>").count(), 1);
    assert_eq!(page.matches("</script>").count(), 1);
}

#[test]
fn rejects_missing_or_oversized_views() {
    let mut content = content();
    content.view = None;
    assert!(document(&content, json!({}), json!({}), false, vec![]).is_err());
    content = super::tests::content();
    content.view.as_mut().unwrap().scripts =
        vec!["a".repeat(sailry_protocol::artifact::MAX_VIEW_BYTES)];
    assert!(document(&content, json!({}), json!({}), false, vec![]).is_err());
    content = super::tests::content();
    content.html = "a".repeat(sailry_protocol::artifact::MAX_HTML_BYTES + 1);
    assert!(document(&content, json!({}), json!({}), false, vec![]).is_err());
}

#[test]
fn supports_template_tags_and_ignores_raw_text() {
    let mut content = content();
    content.view.as_mut().unwrap().template =
        "<!DOCTYPE html><!-- <head> --><HTML><HEAD lang=\"en\" data-note=\">\"><style>/* <head> */</style></HEAD><BODY><script>const sample='</body>';</script><main id=\"brief\"></main></BODY></HTML>".into();
    let page = document(&content, json!({}), json!({}), false, vec![]).unwrap();
    assert!(page.contains(
        "<HEAD lang=\"en\" data-note=\">\"><meta http-equiv=\"Content-Security-Policy\""
    ));
    assert!(page.contains("const sample='</body>';"));
    assert!(page.contains("window.sailryDocument.mount("));
    assert!(page.contains("</script></BODY>"));
    content.view.as_mut().unwrap().template = "<main id=\"brief\"></main>".into();
    assert!(document(&content, json!({}), json!({}), false, vec![]).is_err());
}

#[test]
fn quotes_custom_font_names() {
    assert_eq!(
        font_family("", "system-ui, sans-serif"),
        "system-ui, sans-serif"
    );
    assert_eq!(
        font_family(".SystemUIFont", "system-ui, sans-serif"),
        "system-ui, sans-serif"
    );
    assert_eq!(
        font_family("Example, \"Quoted\" \\ Family\n", "system-ui, sans-serif"),
        "\"Example, \\\"Quoted\\\" \\\\ Family\\a \", system-ui, sans-serif"
    );
}

#[gpui_kit::test]
fn keeps_semantic_colors_in_both_modes(cx: &mut TestAppContext) {
    super::super::super::tests::init(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|cx| {
            Theme::change(mode, None, cx);
            let value = theme(cx);
            let tokens = cx.theme();
            assert_eq!(
                value["background"],
                json!(super::super::super::theme::rgba(tokens.background))
            );
            assert_eq!(
                value["foreground"],
                json!(super::super::super::theme::rgba(tokens.foreground))
            );
            assert_eq!(
                value["primary"],
                json!(super::super::super::theme::rgba(tokens.primary))
            );
            assert_eq!(
                value["success"],
                json!(super::super::super::theme::rgba(tokens.success))
            );
            assert_ne!(value["background"], value["foreground"]);
            assert!(
                value["font_family"]
                    .as_str()
                    .unwrap()
                    .ends_with("system-ui, sans-serif")
            );
            assert!(
                value["mono_font_family"]
                    .as_str()
                    .unwrap()
                    .ends_with("ui-monospace, monospace")
            );
            assert!(
                !value["font_family"]
                    .as_str()
                    .unwrap()
                    .contains(".SystemUIFont")
            );
        });
    }
}
