use super::super::knowledge::tests::fixture::{Fixture, draw, init, tap, wait};
use super::*;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;
use sailry_protocol::Output;

#[gpui::test]
fn autosaves_runtime_settings_on_the_selected_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(super::super::Section::Runtime, cx)
            })
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .runtime
                .panels
                .values()
                .any(|panel| panel.read(cx).settings.is_some() && !panel.read(cx).loading)
        });
        let panel = visual.update(|_, cx| {
            owner
                .read(cx)
                .runtime
                .panels
                .values()
                .next()
                .unwrap()
                .clone()
        });
        draw(visual);
        assert!(visual.debug_bounds("runtime-python-index").is_some());
        assert!(visual.debug_bounds("runtime-settings-save").is_none());
        assert!(visual.debug_bounds("runtime-refresh").is_none());
        visual.update(|window, cx| {
            let index = panel.read(cx).index.clone();
            index.update(cx, |input, cx| {
                input.set_value("https://pypi.example.test/simple/", window, cx);
                cx.emit(InputEvent::Blur);
            });
        });
        wait(visual, |cx| {
            panel
                .read(cx)
                .python
                .as_ref()
                .is_some_and(|settings| settings.revision == 1)
                && !panel.read(cx).pending
        });
        let Output::PythonSettings(settings) = fixture.execute(Command::ReadPythonSettings) else {
            panic!("Python settings expected")
        };
        assert_eq!(
            settings.index.as_deref(),
            Some("https://pypi.example.test/simple/")
        );
        tap(visual, "runtime-ocr-enable");
        wait(visual, |cx| {
            panel
                .read(cx)
                .settings
                .as_ref()
                .is_some_and(|settings| !settings.enabled)
                && !panel.read(cx).pending
        });
        let Output::OcrSettings(settings) = fixture.execute(Command::ReadOcrSettings) else {
            panic!("OCR settings expected")
        };
        assert!(!settings.enabled);
        assert_eq!(settings.revision, 1);
        fixture.close(visual);
    }
}

#[gpui::test]
fn uses_shared_model_rows_and_installed_choices(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(super::super::Section::Runtime, cx)
            })
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .runtime
                .panels
                .values()
                .any(|panel| panel.read(cx).models.len() == 3 && !panel.read(cx).loading)
        });
        let panel = visual.update(|_, cx| {
            owner
                .read(cx)
                .runtime
                .panels
                .values()
                .next()
                .unwrap()
                .clone()
        });
        let id = visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let missing = Some(panel.models[0].id.clone());
                panel.model.update(cx, |state, cx| {
                    state.set_selected_value(&missing, window, cx)
                });
                assert!(panel.model.read(cx).selected_value().is_none());
                panel.models[0].state = Status::Installed;
                panel.choices(window, cx);
                panel.models[0].id.clone()
            })
        });
        draw(visual);
        visual.update(|window, _| {
            assert!(
                window
                    .try_find(SharedString::from(format!("ocr-details-{id}")))
                    .is_some()
            );
            assert!(
                window
                    .try_find(SharedString::from(format!("ocr-install-{id}")))
                    .is_none()
            );
        });
        tap(visual, &format!("ocr-details-{id}"));
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        draw(visual);
        assert!(visual.debug_bounds("model-detail-source").is_some());
        fixture.close(visual);
    }
}

#[gpui::test]
fn recognizes_only_pinned_catalog_sources(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(super::super::Section::Runtime, cx)
            })
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .runtime
                .panels
                .values()
                .any(|panel| !panel.read(cx).models.is_empty() && !panel.read(cx).loading)
        });
        visual.update(|_, cx| {
            let panel = owner
                .read(cx)
                .runtime
                .panels
                .values()
                .next()
                .unwrap()
                .clone();
            panel.update(cx, |panel, _| {
                let installed = &mut panel.models[0];
                installed.definition.source.hub = sailry_protocol::models::Hub::ModelScope;
                installed.definition.source.revision = "master".into();
                installed.state = Status::Installed;
                let definition = installed.definition.clone();
                definition.validate().unwrap();
                assert!(matches!(
                    panel.model_status(&definition),
                    ModelStatus::Installed
                ));
                assert!(matches!(
                    panel.source_status(&definition.source, Kind::Ocr),
                    ModelStatus::Missing
                ));
                let mut updated = definition.clone();
                updated.assets[0].source.revision = "c".repeat(40);
                assert!(matches!(panel.model_status(&updated), ModelStatus::Missing));
                let mut pinned = definition.source;
                pinned.revision = "b".repeat(40);
                panel.models[0].definition.source = pinned.clone();
                assert!(matches!(
                    panel.source_status(&pinned, Kind::Ocr),
                    ModelStatus::Installed
                ));
                assert!(matches!(
                    panel.source_status(&pinned, Kind::Embedding),
                    ModelStatus::Missing
                ));
            });
        });
        fixture.close(visual);
    }
}
