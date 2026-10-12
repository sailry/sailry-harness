use super::*;
use crate::settings::knowledge::tests::fixture::{Fixture, draw, init, input, selected, tap, wait};
use core::prelude::v1::test;
use sailry_protocol::{
    Output,
    knowledge::Model,
    models::{Asset, Format, Pooling},
};
use std::sync::atomic::Ordering;

mod reads;

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
        query_prefix: None,
        document_prefix: None,
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
fn fits_content_and_scrolls_long_results(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    *fixture.transport.catalog.lock().unwrap() = Some(definition());
    let (owner, visual) = fixture.mount(cx);
    let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
    tap(visual, "knowledge-model-library");
    let library =
        visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
    wait(visual, |cx| {
        library.read(cx).items.len() == 1 && !library.read(cx).loading
    });
    let compact = visual.debug_bounds("dialog-0").unwrap();
    let results = visual
        .debug_bounds("settings-group-model_library_results")
        .unwrap();
    assert!(
        compact.bottom() - results.bottom() < px(64.),
        "{compact:?} {results:?}"
    );
    let item = visual.update(|_, cx| library.read(cx).items[0].clone());
    visual.update(|_, cx| {
        library.update(cx, |library, cx| {
            library.items = vec![item; 40];
            cx.notify();
        })
    });
    draw(visual);
    let expanded = visual.debug_bounds("dialog-0").unwrap();
    assert!(expanded.size.height > compact.size.height);
    assert!(expanded.bottom() <= px(1100.));
    let before = visual.debug_bounds("model-library-install-39").unwrap();
    assert!(before.bottom() > expanded.bottom());
    visual.simulate_event(ScrollWheelEvent {
        position: expanded.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(visual);
    let after = visual.debug_bounds("model-library-install-39").unwrap();
    assert!(after.top() < before.top());
    assert!(after.top() >= expanded.top() && after.bottom() <= expanded.bottom());
    visual.update(|_, cx| {
        library.update(cx, |library, cx| {
            library.items.clear();
            library.loading = true;
            cx.notify();
        })
    });
    draw(visual);
    let empty = visual.debug_bounds("dialog-0").unwrap();
    let results = visual
        .debug_bounds("settings-group-model_library_results")
        .unwrap();
    assert!(empty.size.height < expanded.size.height);
    assert!(
        empty.bottom() - results.bottom() < px(64.),
        "{empty:?} {results:?}"
    );
    fixture.close(visual);
}

#[gpui::test]
fn registers_on_each_selected_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let mut candidate = definition();
        candidate.query_prefix = Some("old query: ".into());
        candidate.document_prefix = Some("old passage: ".into());
        *fixture.transport.catalog.lock().unwrap() = Some(candidate.clone());
        fixture.transport.hold_install.store(true, Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            library.read(cx).items.len() == 1 && !library.read(cx).loading
        });
        input(visual, "model-library-query", "encoder");
        tap(visual, "model-library-search");
        wait(visual, |cx| {
            library.read(cx).items.len() == 1 && !library.read(cx).loading
        });
        tap(visual, "model-library-details-0");
        wait(visual, |cx| {
            library.read(cx).definition.as_ref() == Some(&candidate) && !library.read(cx).loading
        });
        visual.update(|_, cx| {
            assert_eq!(
                library.read(cx).query_prefix.read(cx).value(),
                "old query: "
            );
            assert_eq!(
                library.read(cx).document_prefix.read(cx).value(),
                "old passage: "
            );
        });
        input(visual, "model-library-query-prefix", "query: ");
        input(visual, "model-library-document-prefix", "passage: ");
        candidate.query_prefix = Some("query: ".into());
        candidate.document_prefix = Some("passage: ".into());
        let catalog = visual.debug_bounds("dialog-0").unwrap();
        let details = visual.debug_bounds("dialog-1").unwrap();
        let cancel = visual.debug_bounds("model-library-details-close").unwrap();
        let download = visual.debug_bounds("model-library-download").unwrap();
        assert!(cancel.right() < download.left());
        assert!(details.right() - download.right() < px(64.));
        assert!(download.bottom() <= details.bottom());
        assert!(catalog.size.height < px(640.));
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
        assert!(visual.debug_bounds("dialog-1").is_none());
        tap(visual, "model-library-details-0");
        wait(visual, |cx| {
            library.read(cx).definition.is_some() && !library.read(cx).loading
        });
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        draw(visual);
        let close = visual.debug_bounds("model-library-details-close").unwrap();
        visual.simulate_click(close.center(), Modifiers::default());
        draw(visual);
        assert!(visual.debug_bounds("dialog-1").is_none());
        assert!(visual.debug_bounds("dialog-0").is_some());
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

#[gpui::test]
fn loads_defaults_and_pages_online_results(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        *fixture.transport.catalog.lock().unwrap() = Some(definition());
        fixture.transport.catalog_more.store(true, Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            library.read(cx).items.len() == 1 && !library.read(cx).loading
        });
        let source = visual.debug_bounds("model-library-source").unwrap();
        visual.update(|window, cx| {
            assert!(
                window
                    .try_find("model-library-knowledge-model-potion-multi")
                    .is_none()
            );
            assert_eq!(
                library.read(cx).hub.read(cx).selected_value(),
                Some(&Hub::HuggingFace)
            );
            assert_eq!(HubChoice(Hub::HuggingFace).title(), "huggingface.co");
            assert_eq!(HubChoice(Hub::ModelScope).title(), "modelscope.cn");
            let query = window.find("model-library-query").bounds();
            let search = window.find("model-library-search").bounds();
            assert!((source.center().y - query.center().y).abs() <= px(1.));
            assert!((source.center().y - search.center().y).abs() <= px(1.));
            assert!(search.size.width <= search.size.height + px(1.));
        });
        tap(visual, "model-library-more");
        wait(visual, |cx| {
            library.read(cx).items.len() == 2 && !library.read(cx).loading
        });
        visual.update(|_, cx| {
            assert!(library.read(cx).cursor.is_none());
            assert_ne!(
                library.read(cx).items[0].source.revision,
                library.read(cx).items[1].source.revision
            );
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let searches = requests
            .iter()
            .filter_map(|request| {
                if let Command::SearchModels(search) = &request.command {
                    Some(search)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(searches.len(), 2);
        assert!(searches[0].query.is_empty() && searches[0].cursor.is_none());
        assert_eq!(searches[1].cursor.as_deref(), Some("second"));
        drop(requests);
        fixture.close(visual);
    }
}

#[gpui::test]
fn disables_current_install_and_allows_new_revision(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let candidate = definition();
        *fixture.transport.catalog.lock().unwrap() = Some(candidate.clone());
        let Output::KnowledgeModel { model } = fixture.execute(Command::RegisterKnowledgeModel {
            definition: candidate.clone(),
        }) else {
            panic!("registered model expected");
        };
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        wait(visual, |cx| panel.read(cx).definition_for(model).is_some());
        fixture
            .transport
            .states
            .lock()
            .unwrap()
            .insert(model, sailry_protocol::knowledge::ModelState::Installed);
        visual.update(|_, cx| {
            crate::settings::knowledge::tests::fixture::set_state(
                &panel,
                model,
                sailry_protocol::knowledge::ModelState::Installed,
                cx,
            )
        });
        tap(visual, "knowledge-model-library");
        let library =
            visual.update(|_, cx| panel.read(cx).library.as_ref().unwrap().upgrade().unwrap());
        wait(visual, |cx| {
            library.read(cx).items.len() == 1 && !library.read(cx).loading
        });
        let before = fixture.transport.requests.lock().unwrap().len();
        visual.update(|window, cx| {
            assert!(matches!(
                library
                    .read(cx)
                    .target
                    .source_status(&library.read(cx).items[0].source, cx),
                ModelStatus::Installed
            ));
            window.click("model-library-install-0", cx);
        });
        draw(visual);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), before);
        let mut newer = candidate;
        newer.source.revision = "c".repeat(40);
        for asset in &mut newer.assets {
            asset.source.revision = newer.source.revision.clone();
        }
        *fixture.transport.catalog.lock().unwrap() = Some(newer.clone());
        tap(visual, "model-library-search");
        wait(visual, |cx| {
            library.read(cx).items[0].source == newer.source && !library.read(cx).loading
        });
        fixture.transport.hold_install.store(true, Ordering::SeqCst);
        tap(visual, "model-library-install-0");
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
        let pending = fixture.transport.requests.lock().unwrap().len();
        visual.update(|window, cx| window.click("model-library-install-0", cx));
        draw(visual);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), pending);
        let requests = fixture.transport.requests.lock().unwrap();
        assert!(requests.iter().any(|request|matches!(&request.command,Command::ResolveModel {source,..} if source==&newer.source)));
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::RegisterKnowledgeModel { .. }))
                .count(),
            1
        );
        drop(requests);
        fixture.close(visual);
    }
}
