use super::*;
use crate::dictation::Update;

#[gpui::test]
fn missing_model_settings(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("desktop/preferences.json");
        cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(path)));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let navigated = std::rc::Rc::new(std::cell::Cell::new(false));
        let received = navigated.clone();
        visual.update(|window, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if matches!(event, Event::DictationSettings) {
                    received.set(true);
                }
            })
            .detach();
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("Keep draft", window, cx))
            });
        });
        fixture::tap(visual, "dictation-toggle");
        wait(visual, |_| navigated.get());
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep draft"
        );
        assert!(visual.debug_bounds("live-chat-send").is_some());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn appends_bilingual_text(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input("Draft: ");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.accept_dictation(Update::Partial("请检查".into()), window, cx);
                assert_eq!(view.draft(cx).as_ref(), "Draft: 请检查");
                view.accept_dictation(Update::Partial("请检查 API".into()), window, cx);
                assert_eq!(view.draft(cx).as_ref(), "Draft: 请检查 API");
                view.accept_dictation(
                    Update::Finished(Ok("请检查 API response，然后运行 test".into())),
                    window,
                    cx,
                );
                assert_eq!(
                    view.draft(cx).as_ref(),
                    "Draft: 请检查 API response，然后运行 test"
                );
                assert!(!view.pending);
                assert!(view.active().is_none());
            });
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn failure_preserves_existing_input(cx: &mut TestAppContext) {
    fixture::init(cx);
    let fixture = fixture::Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.input.update(cx, |input, cx| {
                input.set_value("Keep this draft", window, cx)
            });
            view.accept_dictation(Update::Finished(Err("dictation_microphone")), window, cx);
            assert_eq!(view.draft(cx).as_ref(), "Keep this draft");
        })
    });
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}

#[gpui::test]
fn stops_and_transcribes(cx: &mut TestAppContext) {
    fixture::init(cx);
    let fixture = fixture::Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("live-chat-send").is_some());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.accept_dictation(Update::Preparing, window, cx);
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("live-chat-send").is_none());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.accept_dictation(Update::Recording, window, cx);
        })
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("dictation-waveform").is_some());
    assert!(visual.debug_bounds("dictation-cancel").is_none());
    assert!(visual.debug_bounds("live-chat-send").is_none());
    fixture::tap(visual, "dictation-toggle");
    assert!(view.read_with(visual, |view, _| view.dictation.phase
        == super::super::dictation::Phase::Transcribing));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("live-chat-send").is_none());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.accept_dictation(Update::Finished(Ok("你好 hello".into())), window, cx);
            assert_eq!(view.draft(cx).as_ref(), "你好 hello");
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("live-chat-send").is_some());
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}
