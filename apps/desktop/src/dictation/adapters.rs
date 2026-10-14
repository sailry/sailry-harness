//! Controller adapters for Kit speech; capture and inference stay local.
mod worker;

use super::{Result, SAMPLE_RATE, Service, capture, models, permission};
use gpui_kit::component::speech::{
    AudioFormat, AudioInput, AudioSink, RecognitionSession, SpeechError, SpeechRecognizer,
    SpeechSink, SpeechState,
};
use gpui_kit::{App, AppContext as _, Entity, Subscription, Task, WeakEntity};
use sailry_link::CancellationToken;
use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};

struct Control {
    stop: CancellationToken,
    cancel: CancellationToken,
}

struct Recognizer {
    captured: RefCell<Option<models::Capture>>,
    language: sailry_speech::Language,
    owner: std::sync::Arc<tokio::sync::Mutex<()>>,
    runtime: tokio::runtime::Handle,
    control: Rc<Control>,
}

struct Microphone {
    selected: Option<String>,
    runtime: tokio::runtime::Handle,
    control: Rc<Control>,
    state: WeakEntity<SpeechState>,
}

struct Session {
    commands: mpsc::Sender<worker::Command>,
    control: Rc<Control>,
    _results: Task<()>,
    finishing: bool,
}

// Each recording gets a fresh Kit state so its captured installation lease
// can move exclusively into the blocking recognizer worker.
pub(crate) fn session(
    captured: models::Capture,
    options: crate::preferences::Dictation,
    runtime: tokio::runtime::Handle,
    cancel: CancellationToken,
    cx: &mut App,
) -> Entity<SpeechState> {
    let control = Rc::new(Control {
        stop: CancellationToken::new(),
        // Ending a Kit session must stop its workers without invalidating
        // the controller's queued Final/Error event. UI cancellation still
        // propagates to this child when the draft or view is released.
        cancel: cancel.child_token(),
    });
    let recognizer = Recognizer {
        captured: RefCell::new(Some(captured)),
        language: options.language,
        owner: Service::shared(cx),
        runtime: runtime.clone(),
        control: control.clone(),
    };
    cx.new(|cx| {
        SpeechState::new(cx)
            .recognizer(recognizer)
            .input(Microphone {
                selected: options.microphone,
                runtime,
                control,
                state: cx.weak_entity(),
            })
            .system_fallback(false)
    })
}

impl SpeechRecognizer for Recognizer {
    fn start(
        &self,
        sink: SpeechSink,
        cx: &mut App,
    ) -> std::result::Result<Box<dyn RecognitionSession>, SpeechError> {
        if self.control.cancel.is_cancelled() {
            return Err(recognition_error("dictation_cancelled"));
        }
        let owner = self
            .owner
            .clone()
            .try_lock_owned()
            .map_err(|_| recognition_error("dictation_busy"))?;
        let captured = self
            .captured
            .borrow_mut()
            .take()
            .ok_or_else(|| recognition_error("dictation_failed"))?;
        let (commands, receiver) = mpsc::channel();
        let (updates, mut results) = tokio::sync::mpsc::unbounded_channel();
        let cancel = self.control.cancel.clone();
        let language = self.language;
        // The blocking worker owns both resources. Dropping the UI session
        // only signals cancellation; it cannot release a running inference.
        self.runtime.spawn_blocking(move || {
            worker::run(captured, language, owner, receiver, updates, cancel);
        });
        let result_cancel = self.control.cancel.clone();
        let result_task = cx.spawn(async move |cx| {
            while let Some(result) = results.recv().await {
                if result_cancel.is_cancelled() {
                    break;
                }
                let ended = matches!(result, worker::Update::Final(_) | worker::Update::Error(_));
                cx.update(|cx| match result {
                    worker::Update::Ready => sink.ready(cx),
                    worker::Update::Partial(text) => sink.hypothesis(text, cx),
                    worker::Update::Final(text) => {
                        // SenseVoice decodes cumulative audio. Commit that
                        // cumulative result once, replacing its hypothesis.
                        sink.phrase(text, cx);
                        sink.finish(cx);
                    }
                    worker::Update::Error(error) => sink.error(recognition_error(error), cx),
                });
                if ended {
                    return;
                }
            }
            if !result_cancel.is_cancelled() {
                cx.update(|cx| sink.error(recognition_error("dictation_failed"), cx));
            }
        });
        Ok(Box::new(Session {
            commands,
            control: self.control.clone(),
            _results: result_task,
            finishing: false,
        }))
    }
}

impl RecognitionSession for Session {
    fn push_audio(&mut self, samples: &[i16], _: &mut App) {
        if !self.finishing && !self.control.cancel.is_cancelled() {
            let _ = self.commands.send(worker::Command::Audio(samples.to_vec()));
        }
    }

    fn finish(&mut self, _: &mut App) {
        if !self.finishing {
            self.finishing = true;
            let _ = self.commands.send(worker::Command::Finish);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.control.cancel.cancel();
        self.control.stop.cancel();
    }
}

enum Audio {
    Samples(Vec<i16>),
    End(Result<()>),
}

impl AudioInput for Microphone {
    fn start(
        &self,
        format: AudioFormat,
        sink: AudioSink,
        cx: &mut App,
    ) -> std::result::Result<Subscription, SpeechError> {
        if self.control.cancel.is_cancelled() {
            return Err(input_error("dictation_cancelled"));
        }
        if format != AudioFormat::new(SAMPLE_RATE, 1) {
            return Err(input_error("dictation_failed"));
        }
        permission::verify().map_err(input_error)?;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let selected = self.selected.clone();
        let stop = self.control.stop.clone();
        let cancel = self.control.cancel.clone();
        self.runtime.spawn_blocking(move || {
            let result = record(selected.as_deref(), &stop, &cancel, &sender);
            let _ = sender.send(Audio::End(result));
        });
        let cancel = self.control.cancel.clone();
        let state = self.state.clone();
        let pump = cx.spawn(async move |cx| {
            while let Some(audio) = receiver.recv().await {
                if cancel.is_cancelled() {
                    break;
                }
                let ended = matches!(audio, Audio::End(_));
                cx.update(|cx| match audio {
                    Audio::Samples(samples) => sink.push(samples, cx),
                    Audio::End(Ok(())) => {
                        let _ = state.update(cx, |state, cx| state.stop(cx));
                    }
                    Audio::End(Err(error)) => sink.error(input_error(error), cx),
                });
                if ended {
                    return;
                }
            }
            if !cancel.is_cancelled() {
                cx.update(|cx| sink.error(input_error("dictation_microphone"), cx));
            }
        });
        let stop = self.control.stop.clone();
        Ok(Subscription::new(move || {
            stop.cancel();
            drop(pump);
        }))
    }
}

fn record(
    selected: Option<&str>,
    stop: &CancellationToken,
    cancel: &CancellationToken,
    sender: &tokio::sync::mpsc::UnboundedSender<Audio>,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Ok(());
    }
    let recorder = capture::Recorder::new(selected)?;
    recorder.start()?;
    while !stop.is_cancelled() && !cancel.is_cancelled() && !recorder.full() {
        recorder.check()?;
        let samples = recorder.take();
        if !samples.is_empty() && sender.send(Audio::Samples(samples)).is_err() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let tail = recorder.finish()?;
    if !cancel.is_cancelled() && !tail.is_empty() {
        let _ = sender.send(Audio::Samples(tail));
    }
    Ok(())
}

#[derive(Debug)]
struct Failure(&'static str);

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for Failure {}

fn input_error(key: &'static str) -> SpeechError {
    SpeechError::input(Failure(key))
}

fn recognition_error(key: &'static str) -> SpeechError {
    SpeechError::recognizer(Failure(key))
}

pub(crate) fn error_key(error: &SpeechError) -> &'static str {
    match error {
        SpeechError::PermissionDenied => "dictation_permission",
        SpeechError::NoInputDevice => "dictation_microphone",
        SpeechError::Unsupported => "dictation_unavailable",
        SpeechError::Input(error) | SpeechError::Recognizer(error) => error
            .downcast_ref::<Failure>()
            .map_or("dictation_failed", |failure| failure.0),
    }
}
