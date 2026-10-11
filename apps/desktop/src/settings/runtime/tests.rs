use super::super::knowledge::tests::fixture::{Fixture, draw, init, input, tap, wait};
use super::*;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;
use sailry_protocol::Output;

mod failures;

#[gpui::test]
fn autosaves_runtime_settings_on_the_selected_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SavePythonSettings(
            sailry_protocol::python::Settings {
                index: Some("https://pypi.initial.test/simple/".into()),
                ..Default::default()
            },
        ));
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
        let index = visual.debug_bounds("runtime-python-index").unwrap();
        let control = visual
            .debug_bounds("settings-control-runtime_python_custom_url")
            .unwrap();
        assert!(index.size.width > px(200.));
        assert_eq!(index.size.width, control.size.width);
        visual.update(|window, _| {
            let input = window.find("runtime-python-index-input");
            assert_eq!(input.value(), Some("https://pypi.initial.test/simple/"));
            assert_eq!(input.bounds().size.width, index.size.width);
        });
        assert!(visual.debug_bounds("runtime-tab-installed").is_none());
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
                .is_some_and(|settings| settings.revision == 2)
                && !panel.read(cx).pending
        });
        let Output::PythonSettings(settings) = fixture.execute(Command::ReadPythonSettings) else {
            panic!("Python settings expected")
        };
        assert_eq!(
            settings.index.as_deref(),
            Some("https://pypi.example.test/simple/")
        );
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(super::super::Section::Ocr, cx)
            })
        });
        wait(visual, |cx| {
            panel.read(cx).active && !panel.read(cx).loading
        });
        assert!(visual.debug_bounds("runtime-python-index").is_none());
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
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(super::super::Section::Runtime, cx)
            })
        });
        wait(visual, |cx| {
            panel.read(cx).active && !panel.read(cx).loading
        });
        let Output::PythonSettings(mut settings) = fixture.execute(Command::ReadPythonSettings)
        else {
            panic!("Python settings expected")
        };
        settings.index = None;
        fixture.execute(Command::SavePythonSettings(settings));
        wait(visual, |cx| {
            panel
                .read(cx)
                .python
                .as_ref()
                .is_some_and(|settings| settings.revision == 3 && settings.index.is_none())
                && !panel.read(cx).loading
        });
        visual.update(|_, cx| {
            assert_eq!(owner.read(cx).runtime.panels.len(), 1);
            assert_eq!(
                panel.read(cx).source.read(cx).selected_value(),
                Some(&Index::Official)
            );
        });
        assert!(visual.debug_bounds("runtime-python-index").is_none());
        assert!(
            visual
                .debug_bounds("runtime-python-source")
                .unwrap()
                .size
                .width
                > px(200.)
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn autosaves_index_presets_and_custom_value(cx: &mut TestAppContext) {
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
                .any(|panel| panel.read(cx).python.is_some() && !panel.read(cx).loading)
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
        visual.update(|window, _| {
            assert_eq!(
                window.find("runtime-python-source").value(),
                Some("pypi.org")
            );
        });
        assert!(visual.debug_bounds("runtime-python-index").is_none());
        for (position, index) in [Index::Ali, Index::Tencent, Index::Tsinghua]
            .into_iter()
            .enumerate()
        {
            tap(visual, "runtime-python-source");
            visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            visual.simulate_keystrokes("down enter");
            wait(visual, |cx| {
                panel.read(cx).python.as_ref().is_some_and(|settings| {
                    settings.revision == position as u64 + 1
                        && settings.index.as_deref() == index.url()
                }) && !panel.read(cx).pending
                    && !panel.read(cx).loading
            });
            let Output::PythonSettings(settings) = fixture.execute(Command::ReadPythonSettings)
            else {
                panic!("Python settings expected")
            };
            assert_eq!(settings.index.as_deref(), index.url());
            visual.update(|window, _| {
                assert_eq!(
                    window.find("runtime-python-source").value(),
                    Some(index.title().as_ref())
                );
            });
            assert!(visual.debug_bounds("runtime-python-index").is_none());
        }
        tap(visual, "runtime-python-source");
        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        visual.simulate_keystrokes("down enter");
        draw(visual);
        assert!(visual.debug_bounds("runtime-python-index").is_some());
        assert_eq!(
            visual.update(|_, cx| panel.read(cx).python.as_ref().unwrap().revision),
            3
        );
        input(
            visual,
            "runtime-python-index-input",
            "https://packages.example.test/simple/",
        );
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            panel.read(cx).python.as_ref().is_some_and(|settings| {
                settings.revision == 4
                    && settings.index.as_deref() == Some("https://packages.example.test/simple/")
            }) && !panel.read(cx).pending
                && !panel.read(cx).loading
        });
        tap(visual, "runtime-python-source");
        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        visual.simulate_keystrokes("up up up up enter");
        wait(visual, |cx| {
            panel
                .read(cx)
                .python
                .as_ref()
                .is_some_and(|settings| settings.revision == 5 && settings.index.is_none())
                && !panel.read(cx).pending
                && !panel.read(cx).loading
        });
        assert!(visual.debug_bounds("runtime-python-index").is_none());
        let Output::PythonSettings(settings) = fixture.execute(Command::ReadPythonSettings) else {
            panic!("Python settings expected")
        };
        assert_eq!(settings.index, None);
        fixture.close(visual);
    }
}

#[gpui::test]
fn switches_compact_tabs_and_shows_empty_models(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.select(super::super::Section::Ocr, cx)
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
        draw(visual);
        let installed = visual.debug_bounds("runtime-tab-installed").unwrap();
        let available = visual.debug_bounds("runtime-tab-available").unwrap();
        let list = visual
            .debug_bounds("settings-surface-runtime_ocr_list")
            .unwrap();
        assert!(available.right() - installed.left() < list.size.width / 2.);
        assert!(list.top() > available.bottom());
        assert!(visual.debug_bounds("empty-runtime_ocr_empty").is_some());
        let toolbar = visual.debug_bounds("runtime-model-toolbar").unwrap();
        let library = visual.debug_bounds("runtime-model-library").unwrap();
        assert_eq!(library.size, size(px(24.), px(24.)));
        assert_eq!(library.right(), toolbar.right());
        // Group's surface is inside the Kit card's one-pixel border.
        assert_eq!(list.right() + px(1.), toolbar.right());
        assert!((library.center().y - available.center().y).abs() < px(1.));
        assert!(installed.left() - list.left() < px(8.));
        assert!(
            visual
                .debug_bounds("settings-title-runtime_ocr_models")
                .is_none()
        );
        assert!(visual.debug_bounds("runtime-python-index").is_none());
        visual.simulate_click(available.center(), Modifiers::default());
        draw(visual);
        assert_eq!(visual.update(|_, cx| panel.read(cx).tab), 1);
        assert!(visual.debug_bounds("empty-runtime_ocr_empty").is_none());
        let models = visual.update(|_, cx| panel.read(cx).models.clone());
        visual.update(|window, _| {
            for model in models {
                assert!(
                    window
                        .try_find(SharedString::from(format!("ocr-install-{}", model.id)))
                        .is_some()
                );
            }
        });
        let installed = visual.debug_bounds("runtime-tab-installed").unwrap();
        visual.simulate_click(installed.center(), Modifiers::default());
        draw(visual);
        assert_eq!(visual.update(|_, cx| panel.read(cx).tab), 0);
        assert!(visual.debug_bounds("empty-runtime_ocr_empty").is_some());
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
                workspace.select(super::super::Section::Ocr, cx)
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
                workspace.select(super::super::Section::Ocr, cx)
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
