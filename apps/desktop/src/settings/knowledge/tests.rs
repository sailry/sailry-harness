use super::*;
use core::prelude::v1::test;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::test::TestWindowExt as _;
use sailry_protocol::{Command, Output};
pub(in crate::settings) mod fixture;
use fixture::{Fixture, draw, init, selected, tap, toast, wait};

#[gpui::test]
fn tabs_filter_models_and_show_empty_lists(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        let installed = visual.debug_bounds("knowledge-tab-installed").unwrap();
        let available = visual.debug_bounds("knowledge-tab-available").unwrap();
        let toolbar = visual.debug_bounds("knowledge-model-toolbar").unwrap();
        let library = visual.debug_bounds("knowledge-model-library").unwrap();
        let list = visual
            .debug_bounds("settings-surface-knowledge_model_list")
            .unwrap();
        assert!(
            visual
                .debug_bounds("settings-heading-knowledge_models")
                .is_none()
        );
        assert!((installed.center().y - library.center().y).abs() <= px(1.));
        assert!(installed.left() - toolbar.left() <= px(4.));
        assert!((toolbar.right() - library.right()).abs() <= px(1.));
        assert!(list.top() > installed.bottom());
        assert!(installed.size.width + available.size.width < list.size.width);
        assert!(visual.debug_bounds("empty-settings_empty").is_some());
        assert!(
            visual
                .debug_bounds("knowledge-model-granite-multi")
                .is_none()
        );

        let available = visual.debug_bounds("knowledge-tab-available").unwrap();
        visual.simulate_click(available.center(), Modifiers::default());
        draw(visual);
        assert_eq!(visual.update(|_, cx| panel.read(cx).tab), 1);
        assert!(visual.debug_bounds("empty-settings_empty").is_none());
        assert!(
            visual
                .debug_bounds("knowledge-model-granite-multi")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("knowledge-install-granite-multi")
                .is_some()
        );

        visual.update(|_, cx| {
            fixture::set_state(
                &panel,
                Model::BgeSmallZh,
                knowledge::ModelState::Installed,
                cx,
            )
        });
        draw(visual);
        assert!(
            visual
                .debug_bounds("knowledge-model-bge-small-zh")
                .is_none()
        );
        let installed = visual.debug_bounds("knowledge-tab-installed").unwrap();
        visual.simulate_click(installed.center(), Modifiers::default());
        draw(visual);
        assert_eq!(visual.update(|_, cx| panel.read(cx).tab), 0);
        assert!(
            visual
                .debug_bounds("knowledge-model-bge-small-zh")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("knowledge-model-granite-multi")
                .is_none()
        );
        assert!(visual.debug_bounds("empty-settings_empty").is_none());
        fixture.close(visual);
    }
}

#[gpui::test]
fn saves_and_observes_selected_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let mut original = fixture.settings();
        original.mirror = Some("https://mirror.example.test".into());
        fixture.execute(Command::SaveKnowledgeSettings(original));
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        assert!(visual.debug_bounds("knowledge-settings-save").is_none());
        assert!(visual.debug_bounds("knowledge-refresh").is_none());
        assert!(visual.debug_bounds("knowledge-mirror").is_none());
        assert!(visual.debug_bounds("knowledge-add").is_none());
        visual.update(|window, cx| {
            let model = panel.read(cx).model.clone();
            model.update(cx, |model, cx| {
                model.set_selected_value(&None, window, cx);
                cx.emit(SelectEvent::Confirm(model.selected_value().cloned()));
            });
        });
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).saved.as_ref().unwrap().model.is_none()
        });
        assert_eq!(
            fixture.settings().mirror.as_deref(),
            Some("https://mirror.example.test")
        );
        assert_eq!(fixture.settings().revision, 2);
        fixture.bind(&owner, visual, true);
        assert_eq!(
            visual.update(|_, cx| selected(&owner, cx)
                .unwrap()
                .read(cx)
                .saved
                .clone()
                .unwrap()),
            Settings::default()
        );
        fixture.bind(&owner, visual, false);
        let mut settings = fixture.settings();
        settings.mirror = None;
        fixture.execute(Command::SaveKnowledgeSettings(settings));
        wait(visual, |cx| {
            panel
                .read(cx)
                .saved
                .as_ref()
                .is_some_and(|saved| saved.revision == 3 && saved.mirror.is_none())
        });
        fixture.close(visual);
    }
}

#[gpui::test]
fn keeps_download_after_details_close(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::KnowledgeModels(models) = fixture.execute(Command::ReadKnowledgeModels) else {
            panic!("models expected")
        };
        let definition = models
            .into_iter()
            .find(|info| info.model == Model::GraniteMulti)
            .unwrap()
            .definition;
        *fixture.transport.catalog.lock().unwrap() = Some(definition.clone());
        fixture
            .transport
            .hold_install
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        visual.update(|window, cx| window.within("knowledge-model-tabs").click(1usize, cx));
        draw(visual);
        assert_eq!(visual.update(|_, cx| panel.read(cx).tab), 1);
        tap(visual, "knowledge-install-granite-multi");
        wait(visual, |cx| {
            panel.read(cx).pending
                && fixture
                    .transport
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| matches!(request.command, Command::InstallKnowledgeModel { .. }))
        });
        let model = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find_map(|request| match request.command {
                Command::InstallKnowledgeModel { model } => Some(model),
                _ => None,
            })
            .unwrap();
        let Output::KnowledgeModels(mut models) = fixture.execute(Command::ReadKnowledgeModels)
        else {
            panic!("models expected")
        };
        let state = knowledge::ModelState::Downloading {
            percent: 45,
            bytes_per_second: 1024,
        };
        fixture
            .transport
            .states
            .lock()
            .unwrap()
            .insert(model, state.clone());
        models
            .iter_mut()
            .find(|info| info.model == model)
            .unwrap()
            .state = state;
        visual.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.models = models;
                cx.notify();
            })
        });
        draw(visual);
        tap(visual, &format!("knowledge-details-{}", model.key()));
        assert!(visual.update(|window, _| {
            window
                .within("dialog")
                .within("gpui_component::progress::progress::Progress")
                .try_find(SharedString::from(format!(
                    "knowledge-progress-{}",
                    model.key()
                )))
                .is_some()
        }));
        assert!(visual.update(|window, _| {
            window
                .within("dialog")
                .try_find(SharedString::from(format!(
                    "knowledge-speed-{}",
                    model.key()
                )))
                .is_some()
        }));
        visual.update(|window, cx| {
            window
                .within("gpui_component::dialog::footer::DialogFooter")
                .click("close", cx)
        });
        draw(visual);
        assert!(visual.update(|_, cx| panel.read(cx).pending));
        tap(visual, &format!("knowledge-cancel-{}", model.key()));
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).request.is_none()
                && panel.read(cx).cancelling.is_none()
        });
        toast(visual, "knowledge_cancel_requested");
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.iter().filter(|request| matches!(request.command, Command::CancelKnowledgeModel { model: candidate } if candidate == model)).count(), 1);
        assert!(
            requests
                .iter()
                .all(|request| request.target == fixture.node.id())
        );
        assert!(requests.iter().any(|request| matches!(&request.command, Command::ResolveModel { source, .. } if source.revision == "HEAD")));
        assert!(requests.iter().any(|request| matches!(&request.command, Command::RegisterKnowledgeModel { definition: pinned } if pinned == &definition)));
        drop(requests);
        fixture.close(visual);
    }
}

#[gpui::test]
fn selects_only_installed_fallbacks(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::KnowledgeModels(models) = fixture.execute(Command::ReadKnowledgeModels) else {
            panic!("models expected")
        };
        let definition = models
            .into_iter()
            .find(|info| info.model == Model::GraniteMulti)
            .unwrap()
            .definition;
        let Output::KnowledgeModel { model: registered } =
            fixture.execute(Command::RegisterKnowledgeModel { definition })
        else {
            panic!("registered model expected")
        };
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let mut models = panel.models.clone();
                models
                    .iter_mut()
                    .find(|info| info.model == Model::BgeSmallZh)
                    .unwrap()
                    .state = knowledge::ModelState::Installed;
                let chosen = panel.update_choices(models, Some(Model::GraniteMulti), window, cx);
                assert_eq!(chosen, Some(Model::BgeSmallZh));
                assert_eq!(
                    panel.model.read(cx).selected_value(),
                    Some(&Some(Model::BgeSmallZh))
                );
                panel.fallback(Some(Model::GraniteMulti), chosen, window, cx);
            })
        });
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).saved.as_ref().unwrap().model == Some(Model::BgeSmallZh)
        });
        assert_eq!(fixture.settings().model, Some(Model::BgeSmallZh));
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let mut models = panel.models.clone();
                for info in &mut models {
                    info.state = knowledge::ModelState::Installed;
                }
                assert_eq!(
                    panel.update_choices(models, Some(Model::BgeSmallZh), window, cx),
                    Some(Model::BgeSmallZh)
                );
                let models = panel.models.clone();
                assert_eq!(
                    panel.update_choices(models, Some(registered), window, cx),
                    Some(registered)
                );
                assert_eq!(
                    panel.model.read(cx).selected_value(),
                    Some(&Some(registered))
                );
            })
        });
        fixture.close(visual);
    }
}

#[gpui::test]
fn prefers_installed_registered_copies(cx: &mut TestAppContext) {
    use super::super::model_library::ModelStatus;
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::KnowledgeModels(models) = fixture.execute(Command::ReadKnowledgeModels) else {
            panic!("models expected")
        };
        let definition = models
            .into_iter()
            .find(|info| info.model == Model::GraniteMulti)
            .unwrap()
            .definition;
        let Output::KnowledgeModel { model } = fixture.execute(Command::RegisterKnowledgeModel {
            definition: definition.clone(),
        }) else {
            panic!("registered model expected")
        };
        *fixture.transport.catalog.lock().unwrap() = Some(definition.clone());
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        visual.update(|_, cx| panel.update(cx, |panel, cx| {
            assert_eq!(panel.models.iter().find(|info| info.model == Model::GraniteMulti).unwrap().state, knowledge::ModelState::Missing);
            panel.models.iter_mut().find(|info| info.model == model).unwrap().state = knowledge::ModelState::Downloading { percent: 38, bytes_per_second: 1024 };
            assert!(matches!(panel.model_status(&definition), ModelStatus::Downloading(progress) if progress.percent == 38.));
            assert!(matches!(panel.source_status(&definition.source, sailry_protocol::models::Kind::Embedding), ModelStatus::Downloading(progress) if progress.percent == 38.));
            panel.models.iter_mut().find(|info| info.model == model).unwrap().state = knowledge::ModelState::Installed;
            assert!(matches!(panel.model_status(&definition), ModelStatus::Installed));
            assert!(matches!(panel.source_status(&definition.source, sailry_protocol::models::Kind::Embedding), ModelStatus::Installed));
            cx.notify();
        }));
        draw(visual);
        tap(visual, &format!("knowledge-details-{}", model.key()));
        tap(visual, "model-details-update");
        toast(visual, "knowledge_up_to_date");
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::InstallKnowledgeModel { .. }))
        );
        assert!(visual.update(|_, cx| panel.read(cx).request.is_none()));
        fixture.close(visual);
    }
}

#[gpui::test]
fn updates_with_resolved_prefixes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::KnowledgeModels(models) = fixture.execute(Command::ReadKnowledgeModels) else {
            panic!("models expected")
        };
        let mut previous = models
            .into_iter()
            .find(|info| info.model == Model::GraniteMulti)
            .unwrap()
            .definition;
        previous.query_prefix = Some("old query: ".into());
        previous
            .assets
            .sort_by(|left, right| left.name.cmp(&right.name));
        let Output::KnowledgeModel { model } = fixture.execute(Command::RegisterKnowledgeModel {
            definition: previous.clone(),
        }) else {
            panic!("registered model expected")
        };
        let mut latest = previous.clone();
        latest.query_prefix = Some("new query: ".into());
        latest.document_prefix = Some("new passage: ".into());
        *fixture.transport.catalog.lock().unwrap() = Some(latest.clone());
        fixture
            .transport
            .hold_install
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        let selection = fixture.settings().model;
        visual.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel
                    .models
                    .iter_mut()
                    .find(|info| info.model == model)
                    .unwrap()
                    .state = knowledge::ModelState::Installed;
                cx.notify();
            })
        });
        draw(visual);
        tap(visual, &format!("knowledge-details-{}", model.key()));
        tap(visual, "model-details-update");
        wait(visual, |cx| {
            panel.read(cx).pending
                && fixture
                    .transport
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| matches!(request.command, Command::InstallKnowledgeModel { .. }))
        });
        let requests = fixture.transport.requests.lock().unwrap();
        assert!(requests.iter().any(|request| matches!(&request.command, Command::RegisterKnowledgeModel { definition } if definition == &latest)));
        let installing = requests
            .iter()
            .find_map(|request| match request.command {
                Command::InstallKnowledgeModel { model } => Some(model),
                _ => None,
            })
            .unwrap();
        drop(requests);
        let Output::KnowledgeModels(models) = fixture.execute(Command::ReadKnowledgeModels) else {
            panic!("models expected")
        };
        assert!(models.iter().any(|info| info.definition == previous));
        assert!(models.iter().any(|info| info.definition == latest));
        assert_eq!(fixture.settings().model, selection);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.cancel(installing, window, cx))
        });
        wait(visual, |cx| !panel.read(cx).locked());
        fixture.close(visual);
    }
}

#[gpui::test]
fn confirms_keyword_fallback_before_model_deletion(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    let (owner, visual) = fixture.mount(cx);
    let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
    visual.update(|_, cx| {
        panel.update(cx, |panel, cx| {
            panel
                .models
                .iter_mut()
                .find(|info| info.model == Model::GraniteMulti)
                .unwrap()
                .state = knowledge::ModelState::Installed;
            cx.notify();
        })
    });
    draw(visual);
    tap(visual, "knowledge-details-granite-multi");
    tap(visual, "model-details-delete");
    let (_, detail) = crate::prompts::tests::wait(visual);
    assert!(detail.contains("Granite Multilingual"));
    assert!(detail.contains("fall back to keyword search"));
    crate::prompts::tests::answer(visual, "settings_cancel");
    assert!(visual.update(|_, cx| panel.read(cx).request.is_none()));
    fixture.close(visual);
}
