use super::*;
use crate::settings::knowledge::tests::fixture::{Fixture, draw, init, input, selected, tap, wait};
use core::prelude::v1::test;
use sailry_protocol::{
    Output,
    knowledge::Model,
    models::{Asset, Format, Pooling},
};
use std::sync::atomic::Ordering;

fn definition() -> Definition {
    let source = Source {
        hub: Hub::HuggingFace,
        repository: "fixture/encoder".into(),
        revision: "a".repeat(40),
    };
    Definition {
        source: source.clone(),
        name: "Fixture encoder".into(),
        license: "MIT".into(),
        format: Format::Transformer {
            dimension: 8,
            pooling: Pooling::Cls,
            token_types: false,
        },
        assets: ["config.json", "model.onnx", "tokenizer.json"]
            .into_iter()
            .map(|name| Asset {
                source: source.clone(),
                path: name.into(),
                name: name.into(),
                bytes: 8,
                sha256: "b".repeat(64),
            })
            .collect(),
    }
}

#[gpui::test]
fn registers_on_each_selected_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let candidate = definition();
        *fixture.transport.catalog.lock().unwrap() = Some(candidate.clone());
        fixture.transport.hold_install.store(true, Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        input(visual, "model-library-query", "encoder");
        tap(visual, "model-library-search");
        wait(visual, |cx| {
            library.read(cx).items.len() == 1 && !library.read(cx).loading
        });
        tap(visual, "model-library-versions-0");
        wait(visual, |cx| {
            library.read(cx).definition.as_ref() == Some(&candidate) && !library.read(cx).loading
        });
        tap(visual, "model-library-download");
        wait(visual, |cx| {
            panel.read(cx).locked()
                && fixture
                    .transport
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| {
                        matches!(
                            request.command,
                            Command::InstallKnowledgeModel {
                                model: Model::Registered { .. }
                            }
                        )
                    })
        });
        let Output::KnowledgeModels(models) = fixture.execute(Command::ReadKnowledgeModels) else {
            panic!("models expected");
        };
        assert!(models.iter().any(|model| model.definition == candidate
            && matches!(model.model, Model::Registered { .. })));
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::RegisterKnowledgeModel { .. }))
                .count(),
            1
        );
        assert!(
            requests
                .iter()
                .all(|request| request.target == fixture.node.id())
        );
        assert!(requests.iter().any(|request| matches!(&request.command, Command::ResolveModel { kind: Kind::Embedding, source } if source.revision == candidate.source.revision)));
        drop(requests);
        // The install is owned by the persistent settings panel, not the catalog overlay.
        visual.update(|window, cx| window.close_dialog(cx));
        draw(visual);
        visual.update(|window, cx| {
            let model = models
                .iter()
                .find(|model| model.definition == candidate)
                .unwrap()
                .model;
            panel.update(cx, |panel, cx| panel.cancel(model, window, cx));
        });
        wait(visual, |cx| !panel.read(cx).locked());
        fixture.close(visual);
    }
}

#[gpui::test]
fn rejects_download_after_host_change(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    *fixture.transport.catalog.lock().unwrap() = Some(definition());
    let (owner, visual) = fixture.mount(cx);
    let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
    tap(visual, "knowledge-model-library");
    let library =
        visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
    visual.update(|_, cx| library.update(cx, |library, _| library.definition = Some(definition())));
    fixture.bind(&owner, visual, true);
    visual.update(|window, cx| library.update(cx, |library, cx| library.download(window, cx)));
    assert!(
        fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| !matches!(request.command, Command::RegisterKnowledgeModel { .. }))
    );
    fixture.close(visual);
}
