use super::*;
use crate::dictation::Update;
use gpui_kit::{
    component::speech::{
        AudioFormat, AudioInput, AudioSink, RecognitionSession, SpeechError, SpeechRecognizer,
        SpeechSink, SpeechState,
    },
    test::TestWindowExt as _,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Default)]
struct Capture {
    audio: RefCell<Option<AudioSink>>,
    speech: RefCell<Option<SpeechSink>>,
    released: Cell<bool>,
}

struct Input(Rc<Capture>);
struct Recognizer(Rc<Capture>);
struct Session(Rc<Capture>);

impl AudioInput for Input {
    fn start(
        &self,
        _: AudioFormat,
        sink: AudioSink,
        _: &mut App,
    ) -> std::result::Result<Subscription, SpeechError> {
        *self.0.audio.borrow_mut() = Some(sink);
        Ok(Subscription::new(|| {}))
    }
}

impl SpeechRecognizer for Recognizer {
    fn start(
        &self,
        sink: SpeechSink,
        _: &mut App,
    ) -> std::result::Result<Box<dyn RecognitionSession>, SpeechError> {
        *self.0.speech.borrow_mut() = Some(sink);
        Ok(Box::new(Session(self.0.clone())))
    }
}

impl RecognitionSession for Session {
    fn push_audio(&mut self, _: &[i16], _: &mut App) {}
    fn finish(&mut self, _: &mut App) {}
}

impl Drop for Session {
    fn drop(&mut self) {
        self.0.released.set(true);
    }
}

fn connect(view: &Entity<View>, visual: &mut VisualTestContext) -> Rc<Capture> {
    let captured = Rc::new(Capture::default());
    visual.update(|window, cx| {
        let speech = cx.new(|cx| {
            SpeechState::new(cx)
                .input(Input(captured.clone()))
                .recognizer(Recognizer(captured.clone()))
                .system_fallback(false)
        });
        view.update(cx, |view, cx| view.bind_dictation(speech, window, cx));
        window.render_frame(cx);
    });
    captured
}

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
fn readonly_change_during_preparation_stops_input(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("desktop/preferences.json");
        cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(path)));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("Keep draft", window, cx));
                view.start_dictation(window, cx);
                assert!(view.dictation.phase == super::super::dictation::Phase::Preparing);
                view.session.as_mut().unwrap().delegation =
                    Some(Box::new(sailry_protocol::Delegation {
                        session: fixture.session.id,
                        turn: sailry_protocol::TurnId::new(),
                        entry: "fixture".into(),
                        index: 0,
                        role: None,
                    }));
            })
        });
        wait(visual, |cx| {
            view.read(cx).dictation.phase == super::super::dictation::Phase::Idle
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep draft"
        );
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(fixture.task_requests(), 0);
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
    for remote in [false, true] {
        for stopping in [false, true] {
            let fixture = fixture::Fixture::with_tools(remote, vec![]);
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| view.read(cx).connected());
            visual.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.input.update(cx, |input, cx| {
                        input.set_value("Keep this draft", window, cx)
                    });
                })
            });
            let captured = connect(&view, visual);
            if stopping {
                visual.update(|_, cx| captured.speech.borrow().as_ref().unwrap().ready(cx));
                wait(visual, |cx| {
                    view.read(cx).dictation.phase == super::super::dictation::Phase::Recording
                });
                fixture::tap(visual, "dictation-toggle");
                assert!(view.read_with(visual, |view, _| view.dictation.phase
                    == super::super::dictation::Phase::Transcribing));
            }
            visual.update(|_, cx| {
                captured
                    .audio
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .error(SpeechError::NoInputDevice, cx);
            });
            wait(visual, |cx| {
                view.read(cx).dictation.phase == super::super::dictation::Phase::Idle
            });
            assert_eq!(
                view.read_with(visual, |view, cx| view.draft(cx).to_string()),
                "Keep this draft"
            );
            assert!(captured.released.get());
            assert_eq!(fixture.task_requests(), 0);
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}

#[gpui::test]
fn stops_and_transcribes(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        assert!(visual.debug_bounds("live-chat-send").is_some());
        let captured = connect(&view, visual);
        wait(visual, |_| true);
        assert!(visual.debug_bounds("live-chat-send").is_none());
        visual.update(|_, cx| {
            captured.speech.borrow().as_ref().unwrap().ready(cx);
            captured
                .audio
                .borrow()
                .as_ref()
                .unwrap()
                .push(vec![16384; 2560], cx);
        });
        wait(visual, |cx| {
            view.read(cx).dictation.phase == super::super::dictation::Phase::Recording
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dictation-waveform").is_some());
            assert!(window.try_find("dictation-cancel").is_none());
            let speech = view.read(cx).dictation.speech.as_ref().unwrap();
            assert!(speech.read(cx).levels().any(|level| level > 0.5));
        });
        let bars: Vec<_> = (0..4)
            .map(|index| {
                visual
                    .debug_bounds(Box::leak(
                        format!("dictation-waveform-bar-{index}").into_boxed_str(),
                    ))
                    .unwrap()
            })
            .collect();
        assert!(bars[2].size.height > bars[0].size.height);
        assert!(bars[3].size.height > bars[1].size.height);
        assert!(visual.debug_bounds("live-chat-send").is_none());
        fixture::tap(visual, "dictation-toggle");
        assert!(view.read_with(visual, |view, _| view.dictation.phase
            == super::super::dictation::Phase::Transcribing));
        visual.executor().advance_clock(Duration::from_secs(1));
        wait(visual, |_| true);
        assert!(view.read_with(visual, |view, _| view.dictation.phase
            == super::super::dictation::Phase::Transcribing));
        assert!(visual.debug_bounds("live-chat-send").is_none());
        visual.update(|_, cx| {
            let speech = captured.speech.borrow();
            let sink = speech.as_ref().unwrap();
            sink.phrase("你好 hello", cx);
            sink.finish(cx);
        });
        wait(visual, |cx| {
            view.read(cx).dictation.phase == super::super::dictation::Phase::Idle
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "你好 hello"
        );
        assert!(visual.debug_bounds("live-chat-send").is_some());
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn timeout_preserves_current_transcript(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let captured = connect(&view, visual);
        visual.update(|_, cx| {
            let speech = captured.speech.borrow();
            let sink = speech.as_ref().unwrap();
            sink.ready(cx);
            sink.hypothesis("Current transcript", cx);
        });
        wait(visual, |cx| {
            view.read(cx).draft(cx).as_ref() == "Current transcript"
        });
        fixture::tap(visual, "dictation-toggle");
        visual.executor().advance_clock(Duration::from_secs(4));
        wait(visual, |cx| {
            view.read(cx).dictation.phase == super::super::dictation::Phase::Idle
        });
        visual.update(|_, cx| {
            let speech = captured.speech.borrow();
            let sink = speech.as_ref().unwrap();
            sink.phrase("Late transcript", cx);
            sink.finish(cx);
        });
        wait(visual, |_| true);
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Current transcript"
        );
        assert!(captured.released.get());
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn cancel_ignores_late_audio_and_transcript(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for stopping in [false, true] {
            let fixture = fixture::Fixture::with_tools(remote, vec![]);
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| view.read(cx).connected());
            visual.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.input
                        .update(cx, |input, cx| input.set_value("Keep draft", window, cx));
                })
            });
            let captured = connect(&view, visual);
            if stopping {
                visual.update(|_, cx| captured.speech.borrow().as_ref().unwrap().ready(cx));
                wait(visual, |cx| {
                    view.read(cx).dictation.phase == super::super::dictation::Phase::Recording
                });
                fixture::tap(visual, "dictation-toggle");
                assert!(view.read_with(visual, |view, _| view.dictation.phase
                    == super::super::dictation::Phase::Transcribing));
            }
            fixture::tap(visual, "dictation-toggle");
            visual.update(|_, cx| {
                captured
                    .audio
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .push(vec![16384; 2560], cx);
                let speech = captured.speech.borrow();
                let sink = speech.as_ref().unwrap();
                sink.phrase("Stale transcript", cx);
                sink.finish(cx);
            });
            wait(visual, |_| true);
            assert_eq!(
                view.read_with(visual, |view, cx| view.draft(cx).to_string()),
                "Keep draft"
            );
            assert!(view.read_with(visual, |view, _| view.dictation.phase
                == super::super::dictation::Phase::Idle));
            assert!(captured.released.get());
            assert_eq!(fixture.task_requests(), 0);
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}

#[gpui::test]
fn edited_span_cancels_late_completion(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let captured = connect(&view, visual);
        visual.update(|_, cx| {
            captured
                .speech
                .borrow()
                .as_ref()
                .unwrap()
                .hypothesis("Partial draft", cx)
        });
        wait(visual, |cx| {
            view.read(cx).draft(cx).as_ref() == "Partial draft"
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("Edited draft", window, cx))
            });
            captured
                .speech
                .borrow()
                .as_ref()
                .unwrap()
                .hypothesis("Later partial", cx);
        });
        wait(visual, |cx| {
            view.read(cx).dictation.phase == super::super::dictation::Phase::Idle
        });
        visual.update(|_, cx| {
            let speech = captured.speech.borrow();
            let sink = speech.as_ref().unwrap();
            sink.phrase("Late final", cx);
            sink.finish(cx);
        });
        wait(visual, |_| true);
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Edited draft"
        );
        assert!(captured.released.get());
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn released_view_discards_queued_results(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let captured = connect(&view, visual);
        let released = view.downgrade();
        visual.update(|window, _| window.remove_window());
        drop(view);
        cx.update(|_| {});
        cx.run_until_parked();
        assert!(released.upgrade().is_none());
        assert!(captured.released.get());
        cx.update(|cx| {
            captured
                .audio
                .borrow()
                .as_ref()
                .unwrap()
                .push(vec![16384; 2560], cx);
            let speech = captured.speech.borrow();
            let sink = speech.as_ref().unwrap();
            sink.phrase("Released transcript", cx);
            sink.finish(cx);
        });
        cx.run_until_parked();
        assert_eq!(fixture.task_requests(), 0);
        fixture.close();
    }
}
