use super::*;
use crate::dictation::{Service, Update};
use gpui_kit::component::button::{Button, ButtonVariants};
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
    finish: CancellationToken,
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
        Button::new("dictation-toggle")
            .ghost()
            .rounded_full()
            .size_8()
            .p_0()
            .when(phase == Phase::Idle, |button| {
                button.icon(Icon::default().path("icons/reicon/microphone.svg"))
            })
            .when(phase == Phase::Recording, |button| {
                button.child(waveform::render(cx))
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
                        view.dictation.finish.cancel();
                        view.dictation.phase = Phase::Transcribing;
                        cx.notify();
                    }
                    Phase::Preparing | Phase::Transcribing => {
                        view.dictation = State::default();
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }

    fn start_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly() || self.dictation.phase != Phase::Idle {
            return;
        }
        let directory = crate::dictation::directory(cx);
        let Some(directory) = directory else {
            crate::feedback::info("", tr("dictation_unavailable").as_ref(), window, cx);
            return;
        };
        let owner = Service::shared(cx);
        let options = crate::preferences::data(cx).dictation.unwrap_or_default();
        let cancel = CancellationToken::new();
        let finish = CancellationToken::new();
        let (updates, mut receiver) = tokio::sync::mpsc::channel(4);
        self.dictation = State {
            phase: Phase::Preparing,
            cancel: cancel.clone(),
            finish: finish.clone(),
            task: None,
            partial: None,
        };
        self.binding.runtime.spawn(async move {
            let result =
                crate::dictation::run(directory, options, owner, finish, cancel, updates.clone())
                    .await;
            let _ = updates.send(Update::Finished(result)).await;
        });
        self.dictation.task = Some(cx.spawn_in(window, async move |view, cx| {
            while let Some(update) = receiver.recv().await {
                let ended = matches!(update, Update::Finished(_));
                if view
                    .update_in(cx, |view, window, cx| {
                        view.accept_dictation(update, window, cx)
                    })
                    .is_err()
                    || ended
                {
                    break;
                }
            }
        }));
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
            Update::Recording => self.dictation.phase = Phase::Recording,
            Update::Partial(text) => self.apply_dictation(text, window, cx),
            Update::Transcribing => self.dictation.phase = Phase::Transcribing,
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
                    Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
                }
                self.dictation.partial = None;
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
                self.dictation.cancel.cancel();
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
