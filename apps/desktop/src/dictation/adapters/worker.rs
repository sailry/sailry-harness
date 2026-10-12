use super::super::{MAX_SAMPLES, SAMPLE_RATE, capture, error_key, models};
use super::Result;
use sailry_link::CancellationToken;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedMutexGuard, mpsc::UnboundedSender};

pub(super) enum Command {
    Audio(Vec<i16>),
    Finish,
}

pub(super) enum Update {
    Ready,
    Partial(String),
    Final(String),
    Error(&'static str),
}

pub(super) fn run(
    captured: models::Capture,
    language: sailry_speech::Language,
    owner: OwnedMutexGuard<()>,
    commands: mpsc::Receiver<Command>,
    updates: UnboundedSender<Update>,
    cancel: CancellationToken,
) {
    run_with(
        captured,
        owner,
        commands,
        updates,
        cancel,
        move |captured| {
            if !captured.ready() {
                return Err("dictation_model_missing");
            }
            let recognizer = captured.recognizer(language)?;
            Ok(move |audio: &[f32]| recognizer.recognize(audio).map_err(error_key))
        },
    );
}

fn run_with<D>(
    captured: models::Capture,
    _owner: OwnedMutexGuard<()>,
    commands: mpsc::Receiver<Command>,
    updates: UnboundedSender<Update>,
    cancel: CancellationToken,
    load: impl FnOnce(&models::Capture) -> Result<D>,
) where
    D: FnMut(&[f32]) -> Result<String>,
{
    let result = (|| {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let mut decode = load(&captured)?;
        let _ = updates.send(Update::Ready);
        let mut audio = Vec::new();
        let mut decoded = 0;
        let mut last_decode = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            match commands.recv_timeout(Duration::from_millis(20)) {
                Ok(Command::Audio(samples)) => {
                    let remaining = MAX_SAMPLES.saturating_sub(audio.len());
                    audio.extend(
                        samples
                            .into_iter()
                            .take(remaining)
                            .map(|sample| f32::from(sample) / 32768.),
                    );
                }
                Ok(Command::Finish) => {
                    capture::validate(&audio)?;
                    let text = decode(&audio)?;
                    return Ok((!cancel.is_cancelled()).then_some(text));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if audio.len() >= decoded + SAMPLE_RATE as usize
                && last_decode.elapsed() >= Duration::from_millis(800)
            {
                decoded = audio.len();
                let text = decode(&audio)?;
                if !text.is_empty() && !cancel.is_cancelled() {
                    let _ = updates.send(Update::Partial(text));
                }
                last_decode = Instant::now();
            }
        }
    })();
    if !cancel.is_cancelled() {
        match result {
            Ok(Some(text)) => {
                let _ = updates.send(Update::Final(text));
            }
            Ok(None) => {}
            Err(error) => {
                let _ = updates.send(Update::Error(error));
            }
        }
    }
    // Captured model and owned recording guard drop only here, after a
    // synchronous decode has returned, even if its session was cancelled.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn missing_model() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing");
        let captured = models::Capture::default_at(path.clone());
        let owner = Arc::new(Mutex::new(()));
        let guard = owner.clone().try_lock_owned().unwrap();
        let (_commands, receiver) = mpsc::channel();
        let (updates, mut results) = tokio::sync::mpsc::unbounded_channel();
        tokio::task::spawn_blocking(move || {
            run(
                captured,
                sailry_speech::Language::Auto,
                guard,
                receiver,
                updates,
                CancellationToken::new(),
            );
        })
        .await
        .unwrap();
        assert!(matches!(
            results.recv().await,
            Some(Update::Error("dictation_model_missing"))
        ));
        assert!(results.recv().await.is_none());
        assert!(owner.try_lock().is_ok());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn final_consumes_all_tail_pcm() {
        let directory = tempfile::tempdir().unwrap();
        let captured = models::Capture::default_at(directory.path().into());
        let owner = Arc::new(Mutex::new(()));
        let guard = owner.clone().try_lock_owned().unwrap();
        let (commands, receiver) = mpsc::channel();
        let (updates, mut results) = tokio::sync::mpsc::unbounded_channel();
        commands.send(Command::Audio(vec![16384, -8192])).unwrap();
        commands.send(Command::Audio(vec![8192, -16384])).unwrap();
        commands.send(Command::Finish).unwrap();
        tokio::task::spawn_blocking(move || {
            run_with(
                captured,
                guard,
                receiver,
                updates,
                CancellationToken::new(),
                |_| {
                    Ok(|audio: &[f32]| {
                        assert_eq!(audio, [0.5, -0.25, 0.25, -0.5]);
                        Ok("Complete transcript".into())
                    })
                },
            );
        })
        .await
        .unwrap();
        assert!(matches!(results.recv().await, Some(Update::Ready)));
        assert!(
            matches!(results.recv().await, Some(Update::Final(text)) if text == "Complete transcript")
        );
        assert!(results.recv().await.is_none());
        assert!(owner.try_lock().is_ok());
    }

    #[tokio::test]
    async fn cancellation_retains_resources_until_decode_exits() {
        let directory = tempfile::tempdir().unwrap();
        let (manager, id, captured) = models::tests::installed(directory.path().into());
        let owner = Arc::new(Mutex::new(()));
        let guard = owner.clone().try_lock_owned().unwrap();
        let (commands, receiver) = mpsc::channel();
        let (updates, mut results) = tokio::sync::mpsc::unbounded_channel();
        let (started, entered) = tokio::sync::oneshot::channel();
        let release = Arc::new(Barrier::new(2));
        let blocked = release.clone();
        let cancel = CancellationToken::new();
        let stopping = cancel.clone();
        commands.send(Command::Audio(vec![16384])).unwrap();
        commands.send(Command::Finish).unwrap();
        let worker = tokio::task::spawn_blocking(move || {
            run_with(captured, guard, receiver, updates, stopping, |_| {
                let mut started = Some(started);
                Ok(move |_: &[f32]| {
                    let _ = started.take().unwrap().send(());
                    blocked.wait();
                    Ok("Late transcript".into())
                })
            });
        });
        entered.await.unwrap();
        cancel.cancel();
        let recording_held = owner.try_lock().is_err();
        let removal = manager.remove(id);
        tokio::task::spawn_blocking(move || release.wait())
            .await
            .unwrap();
        worker.await.unwrap();
        assert!(recording_held);
        assert_eq!(removal, Err("dictation_busy"));
        assert!(owner.try_lock().is_ok());
        manager.remove(id).unwrap();
        assert!(matches!(results.recv().await, Some(Update::Ready)));
        assert!(
            results.recv().await.is_none(),
            "cancelled inference reports no final"
        );
    }
}
