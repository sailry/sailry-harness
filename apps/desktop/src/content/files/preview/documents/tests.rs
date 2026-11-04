use super::*;
use crate::conversation::live::tests::fixture::Fixture;
use core::prelude::v1::test;

#[test]
fn read_and_renderer_contracts() {
    for (scenario, check) in [
        (
            "reads_documents_locally_and_remotely",
            (|_scenario: &str| {
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
                        assert!(
                            previews.contains(&file.mime),
                            "reads documents locally and remotely"
                        );
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
                            let source = std::fs::read(
                                fixture.directory.path().join("project").join(&file.path),
                            )
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
                            assert_eq!(bytes, source, "reads documents locally and remotely");
                            assert_eq!(
                                std::fs::read(
                                    fixture.directory.path().join("project").join(&file.path)
                                )
                                .unwrap(),
                                source,
                                "reads documents locally and remotely"
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
                            assert_eq!(
                                result,
                                Err("artifact_preview_changed"),
                                "reads documents locally and remotely"
                            );
                        });
                    }
                    fixture.close();
                }
            }) as fn(&str),
        ),
        (
            "rejects_invalid_documents",
            (|_scenario: &str| {
                assert_eq!(
                    assets::validate(PDF, b"not a pdf"),
                    Err("artifact_preview_failed"),
                    "rejects invalid documents"
                );
                assert_eq!(
                    assets::validate("application/msword", b"legacy document"),
                    Err("artifact_preview_unavailable"),
                    "rejects invalid documents"
                );
                for mime in [DOCX, XLSX, PPTX] {
                    assert_eq!(
                        assets::validate(mime, b"not a package"),
                        Err("artifact_preview_failed"),
                        "rejects invalid documents"
                    );
                }
            }),
        ),
        (
            "packages_offline_renderer_and_licenses",
            (|_scenario: &str| {
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
                    assert!(
                        bundle.contains_key(file),
                        "packages offline renderer and licenses: missing {file}"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
