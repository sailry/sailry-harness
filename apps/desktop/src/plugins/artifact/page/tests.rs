use super::*;
use core::prelude::v1::test;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::{Theme, ThemeMode};
use sailry_protocol::{SessionId, TurnId, artifact::ViewContent, plugin};

fn content() -> Content {
    Content {
        artifact: Summary {
            kind: sailry_protocol::artifact::Kind::Document,
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
fn rejects_preview_in_privileged_document() {
    let mut content = content();
    content.artifact.kind = sailry_protocol::artifact::Kind::Preview;
    content.html = "<script>window.ipc.postMessage('submit')</script>".into();
    assert!(document(&content, json!({}), json!({}), false, vec![]).is_err());
}

#[gpui_kit::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn wraps_preview_without_business_bridge(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut content = content();
    content.artifact.kind = sailry_protocol::artifact::Kind::Preview;
    content.html = "<!doctype html><html><head><style>body{display:grid}</style></head><body><button onclick='count++'>Count</button><script>let count=0;</script></body></html>".into();
    let html = cx.update(|cx| crate::content::files::html::surface::document(&content.html, cx));
    assert!(html.contains("sandbox=\"allow-scripts\""));
    assert!(html.contains("frame-src about:"));
    assert!(html.contains("frame-src 'none'"));
    assert!(html.contains("&lt;style&gt;body{display:grid}&lt;/style&gt;"));
    assert!(html.contains("&lt;script&gt;let count=0;&lt;/script&gt;"));
    assert!(!html.contains("window.ipc"));
    assert!(!html.contains("sailryDocument"));
    assert!(!html.contains(&content.artifact.session.to_string()));
    assert!(!html.contains("display','block','important'"));
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

#[test]
fn exports_fragments_without_template_columns() {
    let mut content = content();
    let view = content.view.as_mut().unwrap();
    view.template = "<!doctype html><html><head></head><body><aside id=\"contents\"></aside><main id=\"brief\"></main></body></html>".into();
    view.styles = vec!["body{display:grid;grid-template-columns:220px minmax(0,1fr)}main{width:100%;max-width:1008px;margin:auto}".into()];
    let original = content.clone();
    let embedded = document(&content, json!({}), json!({}), false, vec![]).unwrap();
    assert!(embedded.contains("grid-template-columns:220px minmax(0,1fr)"));
    assert!(embedded.contains("<aside id=\"contents\"></aside>"));
    assert!(!embedded.contains("setProperty('display','block'"));
    assert_eq!(content, original);

    let script = serialize_capture(Some(7), gpui_kit::black());
    assert!(script.contains("document.documentElement.cloneNode(true)"));
    assert!(script.contains("const output=clone.querySelector('body')"));
    assert!(script.contains("output.innerHTML=body+"));
    assert!(script.contains("output.style.setProperty('display','block','important')"));
    assert!(script.contains("\\u003cfooter data-sailry-branding style="));
    assert!(script.contains("margin-top:16px;padding:16px"));
    assert!(script.contains("gap:12px"));
    assert!(script.contains("width=32 height=32"));
    assert!(script.contains("width=80 height=80"));
    assert!(script.contains("padding:8px;background:#fff;box-sizing:content-box"));
    assert!(script.contains(&super::super::super::theme::rgba(gpui_kit::black())));
    assert!(!script.contains("document.body.style"));
    assert!(!script.contains("document.body.innerHTML"));
}

#[gpui_kit::test]
fn keeps_semantic_colors_in_both_modes(cx: &mut TestAppContext) {
    super::super::super::tests::init(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|cx| {
            Theme::change(mode, None, cx);
            let value = theme(cx);
            let tokens = cx.theme();
            let script = serialize_capture(Some(8), tokens.foreground);
            assert!(script.contains(&super::super::super::theme::rgba(tokens.foreground)));
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
