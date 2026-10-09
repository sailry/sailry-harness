use super::*;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;

#[gpui::test]
fn language_and_downloads(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
    });
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(Panel::new);
        view.update(cx, |this, _| {
            this.state = State::Missing;
            this.microphones = vec![("fixture-device-id".into(), "USB microphone".into())];
        });
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.unwrap();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("dictation-download").is_some());
    let microphone = visual.debug_bounds("dictation-microphone").unwrap();
    visual.simulate_click(microphone.center(), Modifiers::default());
    visual.run_until_parked();
    visual.simulate_keystrokes("down down enter");
    visual.run_until_parked();
    visual.update(|_, cx| {
        assert_eq!(
            preferences::data(cx)
                .dictation
                .unwrap()
                .microphone
                .as_deref(),
            Some("fixture-device-id")
        );
    });
    let language = visual.debug_bounds("dictation-language").unwrap();
    visual.simulate_click(language.center(), Modifiers::default());
    visual.run_until_parked();
    visual.simulate_keystrokes("down down down enter");
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert_eq!(
            preferences::data(cx).dictation.unwrap().language,
            preferences::Language::English
        );
        panel.update(cx, |this, cx| {
            this.accept(
                Update::Progress(dictation::model::DownloadProgress {
                    percent: 35.,
                    bytes_per_second: 2_500_000.,
                }),
                window,
                cx,
            )
        });
        window.draw(cx).clear(cx);
    });
    let cancel = visual.debug_bounds("dictation-download-cancel").unwrap();
    assert!(visual.debug_bounds("dictation-download-progress").is_some());
    assert!(visual.debug_bounds("dictation-download-speed").is_some());
    visual.simulate_click(cancel.center(), Modifiers::default());
    visual.update(|window, cx| {
        panel.update(cx, |this, cx| {
            assert!(this.cancel.is_cancelled());
            assert!(!this.permission_stop.is_cancelled());
            this.accept(Update::Finished(Err("dictation_cancelled")), window, cx);
            assert!(this.state == State::Missing);
            this.accept(Update::Finished(Err("dictation_download")), window, cx);
            assert!(this.state == State::Failed);
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("dictation-download").is_some());
    visual.update(|window, cx| {
        panel.update(cx, |this, cx| {
            this.accept(Update::Finished(Ok(())), window, cx)
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("dictation-download").is_none());
    assert!(visual.debug_bounds("dictation-model-status").is_some());
}

#[gpui::test]
fn preview_isolation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
        let panel = cx.new(Panel::new);
        panel.update(cx, |panel, cx| {
            panel.refresh(cx);
            assert!(panel.state == State::Unavailable);
            assert!(panel.task.is_none());
        });
    });
}

#[gpui::test]
fn preview_authorization_does_not_request_access(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(Panel::new);
        view.update(cx, |panel, cx| {
            panel.permission = Status::NotDetermined;
            panel.authorize(window, cx);
        });
        Root::new(view, window, cx)
    });
    visual.run_until_parked();
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(visual.opened_url().is_none());
}

#[gpui::test]
fn permission_guidance(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
    });
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(Panel::new);
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.unwrap();
    for status in [
        Status::NotDetermined,
        Status::Denied,
        Status::Restricted,
        Status::Granted,
    ] {
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.permission = status;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        let selector = match status {
            Status::NotDetermined => "dictation-access-NotDetermined",
            Status::Denied => "dictation-access-Denied",
            Status::Restricted => "dictation-access-Restricted",
            Status::Granted => "dictation-access-Granted",
            Status::Unknown => "dictation-access-Unknown",
        };
        assert_eq!(
            visual.debug_bounds(selector).is_some(),
            status == Status::Granted && permission::settings_url().is_some()
        );
        assert_eq!(
            visual.debug_bounds("dictation-authorize").is_some(),
            status != Status::Granted && permission::settings_url().is_some()
        );
    }
}

#[gpui::test]
fn authorization_settings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        let panel = cx.new(Panel::new);
        // A rejected system request can leave the status undetermined when
        // the launching application's TCC identity prohibits microphone use.
        panel.update(cx, |panel, cx| {
            panel.finish_authorization(Status::NotDetermined, cx);
            assert_eq!(panel.permission, Status::NotDetermined);
        });
    });
    // Returning from a request does not open another OS flow automatically.
    assert!(cx.opened_url().is_none());
}

#[gpui::test]
fn granted_authorization(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        let panel = cx.new(Panel::new);
        panel.update(cx, |panel, cx| {
            panel.finish_authorization(Status::Granted, cx);
            assert_eq!(panel.permission, Status::Granted);
        });
    });
    assert!(cx.opened_url().is_none());
}

#[gpui::test]
fn switches_persisted_controller_models(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let id = sailry_protocol::ModelId::new();
    let source = sailry_protocol::models::Source {
        hub: sailry_protocol::models::Hub::HuggingFace,
        repository: "fixture/sensevoice".into(),
        revision: "a".repeat(40),
    };
    let definition = sailry_protocol::models::Definition {
        source: source.clone(),
        name: "Controller speech".into(),
        license: "Apache-2.0".into(),
        query_prefix: None,
        document_prefix: None,
        format: sailry_protocol::models::Format::SenseVoice {},
        assets: ["model.onnx", "tokens.txt"]
            .into_iter()
            .map(|name| sailry_protocol::models::Asset {
                source: source.clone(),
                path: name.into(),
                name: name.into(),
                bytes: 4,
                sha256: "b".repeat(64),
            })
            .collect(),
    };
    std::fs::create_dir(directory.path().join("models")).unwrap();
    std::fs::write(
        directory.path().join("models/speech.json"),
        serde_json::to_vec(&vec![dictation::models::Record {
            id,
            definition,
            installed: true,
        }])
        .unwrap(),
    )
    .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(preferences::Preferences::open(
            directory.path().join("preferences.json"),
        ));
        cx.set_global(dictation::Service::default());
    });
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(Panel::new);
        view.update(cx, |panel, cx| panel.bind(None, window, cx));
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.unwrap();
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| {
        let selector = SharedString::from(format!("speech-select-{id}"));
        assert_ne!(window.find(selector.clone()).disabled(), Some(true));
        window.click(selector, cx);
    });
    visual.run_until_parked();
    assert_eq!(
        preferences::Preferences::open(directory.path().join("preferences.json"))
            .data
            .dictation
            .unwrap()
            .model,
        Some(id)
    );
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual.debug_bounds("speech-default-select").unwrap();
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert_eq!(preferences::data(cx).dictation.unwrap().model, None);
        panel.update(cx, |panel, cx| panel.remove(id, window, cx));
        assert!(dictation::Service::models(cx).unwrap().entries().is_empty());
        window.remove_window();
    });
    assert_eq!(
        preferences::Preferences::open(directory.path().join("preferences.json"))
            .data
            .dictation
            .unwrap()
            .model,
        None
    );
    assert!(
        dictation::models::Manager::open(directory.path().into())
            .unwrap()
            .entries()
            .is_empty()
    );
}
