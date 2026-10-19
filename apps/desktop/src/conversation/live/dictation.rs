use super::*;
use crate::dictation::Update;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::speech::{SpeechEvent, SpeechState, SpeechStatus};
use gpui_kit::prelude::FluentBuilder as _;

mod waveform;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Idle,
    Preparing,
    Recording,
    Transcribing,
}
#[derive(Default)]
pub(super) struct State {
    pub(super) phase: Phase,
    pub(super) speech: Option<Entity<SpeechState>>,
    events: Option<Subscription>,
    observer: Option<Subscription>,
    cancel: CancellationToken,
    task: Option<Task<()>>,
    partial: Option<(std::ops::Range<usize>, String)>,
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl View {
    pub(super) fn dictation_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let phase = self.dictation.phase;
        let label = match phase {
            Phase::Idle => "dictation_start",
            Phase::Preparing => "dictation_preparing",
            Phase::Recording => "dictation_finish",
            Phase::Transcribing => "dictation_transcribing",
        };
        // SpeechButton starts before controller consent/model preflight and
        // ignores stopping clicks. Kit Button keeps the approved cancel action
        // and waveform child while SpeechState owns capture and recognition.
        Button::new("dictation-toggle")
            .ghost()
            .rounded_full()
            .size_8()
            .p_0()
            .when(phase == Phase::Idle, |button| {
                button.icon(Icon::default().path("icons/reicon/microphone.svg"))
            })
            .when(phase == Phase::Recording, |button| {
                button.children(
                    self.dictation
                        .speech
                        .as_ref()
                        .map(|speech| waveform::render(speech, cx)),
                )
            })
            .when(
                matches!(phase, Phase::Preparing | Phase::Transcribing),
                |button| button.icon(gpui_kit::component::spinner::Spinner::new()),
            )
            .selected(phase == Phase::Recording)
            .disabled(self.readonly())
            .debug_selector(|| "dictation-toggle".into())
            .tooltip(tr(label))
            .accessibility_label(tr(label))
            .on_click(
                cx.listener(|view, _, window, cx| match view.dictation.phase {
                    Phase::Idle => view.start_dictation(window, cx),
                    Phase::Recording => {
                        if let Some(speech) = &view.dictation.speech {
                            speech.update(cx, |speech, cx| speech.stop(cx));
                        }
                        view.dictation.phase = Phase::Transcribing;
                        cx.notify();
                    }
                    Phase::Preparing | Phase::Transcribing => view.cancel_dictation(cx),
                }),
            )
            .into_any_element()
    }

    pub(super) fn start_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly() || self.dictation.phase != Phase::Idle {
            return;
        }
        let captured = match crate::dictation::capture(cx) {
            Ok(captured) => captured,
            Err("dictation_model_missing") => {
                cx.emit(Event::DictationSettings);
                return;
            }
            Err(error) => {
                crate::feedback::error("", tr(error).as_ref(), window, cx);
                return;
            }
        };
        let cancel = CancellationToken::new();
        let options = crate::preferences::data(cx).dictation.unwrap_or_default();
        self.dictation = State {
            phase: Phase::Preparing,
            cancel: cancel.clone(),
            speech: None,
            events: None,
            observer: None,
            task: None,
            partial: None,
        };
        let check = captured.clone();
        let ready = cx.background_executor().spawn(async move { check.ready() });
        self.dictation.task = Some(cx.spawn_in(window, async move |view, cx| {
            let ready = ready.await;
            let _ = view.update_in(cx, |view, window, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                if view.readonly() {
                    view.dictation = State::default();
                    cx.notify();
                    return;
                }
                if !ready {
                    view.accept_dictation(
                        Update::Finished(Err("dictation_model_missing")),
                        window,
                        cx,
                    );
                    return;
                }
                let owner = cx.entity().downgrade();
                crate::permissions::microphone(
                    cancel.clone(),
                    Box::new(move |granted, window, cx| {
                        let _ = owner.update(cx, |view, cx| {
                            if cancel.is_cancelled() {
                                return;
                            }
                            if granted && !view.readonly() {
                                view.record_dictation(captured, options, cancel, window, cx);
                            } else {
                                view.dictation = State::default();
                                cx.notify();
                            }
                        });
                    }),
                    window,
                    cx,
                );
            });
        }));
        cx.notify();
    }

    fn record_dictation(
        &mut self,
        captured: crate::dictation::models::Capture,
        options: crate::preferences::Dictation,
        cancel: CancellationToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let speech = crate::dictation::session(
            captured,
            options,
            self.binding.runtime.handle().clone(),
            cancel,
            cx,
        );
        self.bind_dictation(speech, window, cx);
    }

    pub(super) fn bind_dictation(
        &mut self,
        speech: Entity<SpeechState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let generation = speech.entity_id();
        let cancel = self.dictation.cancel.clone();
        self.dictation.events = Some(cx.subscribe_in(
            &speech,
            window,
            move |view, _, event: &SpeechEvent, window, cx| {
                if cancel.is_cancelled()
                    || view.dictation.speech.as_ref().map(Entity::entity_id) != Some(generation)
                {
                    return;
                }
                match event {
                    SpeechEvent::Started => view.accept_dictation(Update::Preparing, window, cx),
                    SpeechEvent::Partial(text) => {
                        view.accept_dictation(Update::Partial(text.to_string()), window, cx)
                    }
                    SpeechEvent::Final(text) => {
                        view.accept_dictation(Update::Finished(Ok(text.to_string())), window, cx)
                    }
                    SpeechEvent::Error(error) => view.accept_dictation(
                        Update::Finished(Err(crate::dictation::speech_error_key(error))),
                        window,
                        cx,
                    ),
                    SpeechEvent::Cancelled => {
                        view.dictation = State::default();
                        cx.notify();
                    }
                }
            },
        ));
        self.dictation.observer = Some(cx.observe(&speech, move |view, speech, cx| {
            if view.dictation.speech.as_ref().map(Entity::entity_id) != Some(generation) {
                return;
            }
            view.dictation.phase = match speech.read(cx).status() {
                SpeechStatus::Idle => Phase::Idle,
                SpeechStatus::Connecting => Phase::Preparing,
                SpeechStatus::Recording => Phase::Recording,
                SpeechStatus::Stopping => Phase::Transcribing,
            };
            cx.notify();
        }));
        self.dictation.speech = Some(speech.clone());
        speech.update(cx, |speech, cx| speech.start(cx));
        cx.notify();
    }

    fn cancel_dictation(&mut self, cx: &mut Context<Self>) {
        self.dictation.cancel.cancel();
        if let Some(speech) = &self.dictation.speech {
            speech.update(cx, |speech, cx| speech.cancel(cx));
        }
        self.dictation = State::default();
        cx.notify();
    }

    pub(super) fn accept_dictation(
        &mut self,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match update {
            Update::Preparing => self.dictation.phase = Phase::Preparing,
            Update::Partial(text) => self.apply_dictation(text, window, cx),
            Update::Finished(result) => {
                self.dictation.phase = Phase::Idle;
                if self.dictation.cancel.is_cancelled() {
                    self.dictation.partial = None;
                    cx.notify();
                    return;
                }
                match result {
                    Ok(text) if !text.trim().is_empty() && !self.readonly() => {
                        self.apply_dictation(text, window, cx);
                    }
                    Ok(_) => crate::feedback::info("", tr("dictation_empty").as_ref(), window, cx),
                    Err("dictation_model_missing") => cx.emit(Event::DictationSettings),
                    Err(error) => crate::feedback::error("", tr(error).as_ref(), window, cx),
                }
                self.dictation.partial = None;
                self.dictation.speech = None;
                self.dictation.events = None;
                self.dictation.observer = None;
            }
        }
        cx.notify();
    }

    fn apply_dictation(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if text.is_empty() || self.readonly() || self.dictation.cancel.is_cancelled() {
            return;
        }
        let range = if let Some((range, previous)) = &self.dictation.partial {
            if self.input.read(cx).value().get(range.clone()) != Some(previous.as_str()) {
                self.cancel_dictation(cx);
                crate::feedback::info("", tr("dictation_draft_changed").as_ref(), window, cx);
                return;
            }
            range.clone()
        } else {
            let cursor = self.input.read(cx).cursor();
            cursor..cursor
        };
        self.input.update(cx, |input, cx| {
            input.set_selected_range(range.clone(), cx);
            input.replace(text.clone(), window, cx);
        });
        self.dictation.partial = Some((range.start..range.start + text.len(), text));
    }
}
