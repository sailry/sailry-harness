use super::super::tests::fixture::{Fixture, init, selected, tap, wait};
use super::*;
use core::prelude::v1::test;
use gpui::TestAppContext;
use sailry_protocol::Command;

#[gpui::test]
fn selects_registered_defaults_and_collection_overrides(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let source = sailry_protocol::models::Source {
            hub: sailry_protocol::models::Hub::ModelScope,
            repository: "fixture/encoder".into(),
            revision: "master".into(),
        };
        let candidate = sailry_protocol::models::Definition {
            source: source.clone(),
            name: "Registered encoder".into(),
            license: "MIT".into(),
            query_prefix: None,
            document_prefix: None,
            format: sailry_protocol::models::Format::Static { dimension: 256 },
            assets: ["config.json", "model.safetensors", "tokenizer.json"]
                .into_iter()
                .map(|name| sailry_protocol::models::Asset {
                    source: sailry_protocol::models::Source {
                        revision: "a".repeat(40),
                        ..source.clone()
                    },
                    path: name.into(),
                    name: name.into(),
                    bytes: 8,
                    sha256: "b".repeat(64),
                })
                .collect(),
        };
        let sailry_protocol::Output::KnowledgeModel { model } =
            fixture.execute(Command::RegisterKnowledgeModel {
                definition: candidate,
            })
        else {
            panic!("registered model expected");
        };
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        visual.update(|window, cx| {
            panel.read(cx).model.clone().update(cx, |state, cx| {
                state.set_selected_value(&Some(model), window, cx)
            });
            assert_eq!(panel.read(cx).settings(cx).unwrap().model, Some(model));
        });
        tap(visual, "knowledge-settings-save");
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).saved.as_ref().unwrap().model == Some(model)
        });
        assert_eq!(fixture.settings().model, Some(model));
        let editor = visual.update(|window, cx| editor::open(panel.clone(), None, window, cx));
        visual.update(|window, cx| {
            editor.read(cx).model.clone().update(cx, |state, cx| {
                state.set_selected_value(&Some(model), window, cx)
            });
            assert_eq!(
                editor.read(cx).model.read(cx).selected_value(),
                Some(&Some(model))
            );
            editor.read(cx).path.clone().update(cx, |state, cx| {
                state.set_value(fixture.source().to_str().unwrap(), window, cx)
            });
            editor
                .read(cx)
                .name
                .clone()
                .update(cx, |state, cx| state.set_value("Pinned notes", window, cx));
            assert_eq!(editor.read(cx).definition(cx).unwrap().model, Some(model));
        });
        tap(visual, "knowledge-editor-save");
        wait(visual, |cx| editor.read(cx).closed);
        let collection = fixture
            .collections()
            .into_iter()
            .find(|collection| collection.definition.name == "Pinned notes")
            .unwrap();
        assert_eq!(collection.definition.model, Some(model));
        let mut settings = fixture.settings();
        settings.model = None;
        fixture.execute(Command::SaveKnowledgeSettings(settings));
        assert_eq!(
            fixture
                .collections()
                .into_iter()
                .find(|current| current.id == collection.id)
                .unwrap()
                .definition
                .model,
            Some(model)
        );
        fixture.close(visual);
    }
}
