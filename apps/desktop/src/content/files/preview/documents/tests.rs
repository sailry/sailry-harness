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
                let bytes = if mime == PDF {
                    read(
                        client.clone(),
                        worktree,
                        file.clone(),
                        Some(context.clone()),
                        CancellationToken::new(),
                        LIMIT,
                    )
                    .await
                    .unwrap()
                } else {
                    converted(
                        client.clone(),
                        worktree,
                        file.clone(),
                        Some(context.clone()),
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap()
                    .0
                };
                assets::validate(PDF, &bytes).unwrap();
                assert_eq!(
                    std::fs::read(fixture.directory.path().join("project").join(&file.path))
                        .unwrap(),
                    source
                );
                let mut stale = file.clone();
                stale.revision = Some("0".repeat(64));
                let result = if mime == PDF {
                    read(
                        client.clone(),
                        worktree,
                        stale,
                        Some(context.clone()),
                        CancellationToken::new(),
                        LIMIT,
                    )
                    .await
                } else {
                    converted(
                        client.clone(),
                        worktree,
                        stale,
                        Some(context.clone()),
                        CancellationToken::new(),
                    )
                    .await
                    .map(|v| v.0)
                };
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
    ] {
        assert!(bundle.contains_key(file), "missing {file}");
    }
    assert!(bundle.keys().all(|file| file.starts_with("pdfjs-dist/")));
}
