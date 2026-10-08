use super::*;
use crate::conversation::live::tests::fixture::Fixture;
use core::prelude::v1::test;

#[test]
fn reads_documents_locally_and_remotely() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/node-runtime/src/office/fixtures.json"
    )))
    .unwrap();
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let context = fixture.files_context();
        let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: "files".into(),
        }) else {
            panic!("Files package expected");
        };
        let previews = info.extension.unwrap().desktop.unwrap().previews;
        for case in &cases {
            let path = case["path"].as_str().unwrap();
            let mime = match path.rsplit('.').next().unwrap() {
                "docx" => DOCX,
                "xlsx" => XLSX,
                "pptx" => PPTX,
                _ => PDF,
            };
            let file = File {
                path: path.into(),
                name: None,
                mime: mime.into(),
                size: None,
                revision: None,
            };
            assert!(previews.contains(&file.mime));
            fixture.runtime.block_on(async {
                let client = fixture.binding.client.clone();
                let worktree = fixture.binding.worktree;
                std::fs::copy(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../crates/node-runtime/src/office/fixtures")
                        .join(path),
                    fixture.directory.path().join("project").join(path),
                )
                .unwrap();
                let source =
                    std::fs::read(fixture.directory.path().join("project").join(&file.path))
                        .unwrap();
                let bytes = read(
                    client.clone(),
                    worktree,
                    file.clone(),
                    Some(context.clone()),
                    CancellationToken::new(),
                    LIMIT,
                )
                .await
                .unwrap();
                assets::validate(mime, &bytes).unwrap();
                assert_eq!(bytes, source);
                assert_eq!(
                    std::fs::read(fixture.directory.path().join("project").join(&file.path))
                        .unwrap(),
                    source
                );
                let mut stale = file.clone();
                stale.revision = Some("0".repeat(64));
                let result = read(
                    client.clone(),
                    worktree,
                    stale,
                    Some(context.clone()),
                    CancellationToken::new(),
                    LIMIT,
                )
                .await;
                assert_eq!(result, Err("artifact_preview_changed"));
            });
        }
        fixture.close();
    }
}

#[test]
fn rejects_invalid_documents() {
    assert_eq!(
        assets::validate(PDF, b"not a pdf"),
        Err("artifact_preview_failed")
    );
    assert_eq!(
        assets::validate("application/msword", b"legacy document"),
        Err("artifact_preview_unavailable")
    );
    for mime in [DOCX, XLSX, PPTX] {
        assert_eq!(
            assets::validate(mime, b"not a package"),
            Err("artifact_preview_failed")
        );
    }
}

#[test]
fn packages_offline_renderer_and_licenses() {
    let bundle = assets::bundle().unwrap();
    for file in [
        "pdfjs-dist/legacy/build/pdf.mjs",
        "pdfjs-dist/legacy/build/pdf.worker.mjs",
        "pdfjs-dist/legacy/web/pdf_viewer.mjs",
        "pdfjs-dist/legacy/web/pdf_viewer.css",
        "pdfjs-dist/LICENSE",
        "docx-preview/dist/docx-preview.min.js",
        "docx-preview/LICENSE",
        "jszip/dist/jszip.min.js",
        "jszip/LICENSE.markdown",
        "xlsx/xlsx.mjs",
        "xlsx/LICENSE",
        "@jvmr/pptx-to-html/dist/index.js",
        "@jvmr/pptx-to-html/LICENSE",
    ] {
        assert!(bundle.contains_key(file), "missing {file}");
    }
}

#[gpui_kit::test]
fn failed_reload_uses_toast_and_preserves_status(cx: &mut TestAppContext) {
    use crate::conversation::live::tests::{fixture, wait};

    fixture::init(cx);
    let fixture = Fixture::with_tools(false, Vec::new());
    let context = fixture.files_context();
    std::fs::write(
        fixture.directory.path().join("project/broken.docx"),
        b"invalid",
    )
    .unwrap();
    let file = File {
        path: "broken.docx".into(),
        name: None,
        mime: DOCX.into(),
        size: None,
        revision: None,
    };
    let mut document = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let source = cx.new(|cx| {
            Chat::new(
                fixture.binding.clone(),
                Some(fixture.session.clone()),
                window,
                cx,
            )
        });
        let view = cx.new(|cx| {
            Document::new(
                fixture.binding.clone(),
                file,
                source.downgrade(),
                Some(context),
                cx,
            )
        });
        document = Some(view.clone());
        Root::new(view, window, cx)
    });
    let document = document.unwrap();
    wait(visual, |cx| {
        document.read(cx).error == Some("artifact_preview_failed")
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert_eq!(
            crate::feedback::tests::count(window, &tr("artifact_preview_failed"), cx),
            1
        );
        document.update(cx, |view, cx| {
            view.status = Status {
                pages: 2,
                page: 1,
                scale: 0.5,
                ..Default::default()
            };
            view.load(cx);
            assert_eq!(view.status.pages, 2);
            cx.notify();
        });
    });
    wait(visual, |cx| {
        document.read(cx).error == Some("artifact_preview_failed")
    });
    visual.run_until_parked();
    document.read_with(visual, |view, _| {
        assert_eq!(view.status.pages, 2);
        assert_eq!(view.status.page, 1);
        assert_eq!(view.status.scale, 0.5);
    });
    visual.update(|window, cx| {
        assert_eq!(
            crate::feedback::tests::count(window, &tr("artifact_preview_failed"), cx),
            2
        );
        window.remove_window();
    });
    drop(document);
    fixture.close();
}
