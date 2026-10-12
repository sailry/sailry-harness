use super::*;
use sailry_protocol::installation::{ModelRef, Phase, State as Status, Target};

fn task(manager: &Manager, id: Option<ModelId>) -> RequestId {
    let request = RequestId::new();
    let mut data = manager.data.lock().unwrap();
    data.requests.insert(id, request);
    data.jobs.insert(
        id,
        Job {
            cancel: CancellationToken::new(),
            state: State::Downloading(Progress::default()),
        },
    );
    data.installations.insert(
        request,
        sailry_protocol::installation::Task {
            request,
            revision: 1,
            resource: ModelRef::Speech {
                target: Target::Controller,
                id,
            }
            .resource(),
            state: Status::Queued,
        },
    );
    request
}

#[test]
fn final_validation_controls_success() {
    for custom in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let manager = Manager::open(directory.path().into()).unwrap();
        let id = custom.then(|| manager.register(definition()).unwrap());
        let request = task(&manager, id);
        manager.progress(
            id,
            Progress {
                phase: sailry_model_assets::Phase::Downloading,
                percent: 100.,
                bytes_per_second: 512.,
            },
        );
        assert!(manager.outcome(request).is_none());
        assert!(!manager.installations().is_empty());
        manager.progress(
            id,
            Progress {
                phase: sailry_model_assets::Phase::Validating,
                ..Default::default()
            },
        );
        assert!(matches!(
            manager.installations()[0].state,
            Status::Running {
                phase: Phase::Validating,
                progress: None
            }
        ));
        manager.finish(
            id,
            Err("dictation_model_invalid"),
            &CancellationToken::new(),
        );
        assert_eq!(
            manager.outcome(request),
            Some(Err("dictation_model_invalid"))
        );
        assert!(matches!(
            manager.installations()[0].state,
            Status::Failed { .. }
        ));
        manager.dismiss(request).unwrap();
        assert!(manager.installations().is_empty());
        assert_eq!(
            manager.outcome(request),
            Some(Err("dictation_model_invalid"))
        );
    }
}

#[test]
fn completion_removes_only_its_task() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let id = manager.register(definition()).unwrap();
    let default = task(&manager, None);
    let custom = task(&manager, Some(id));
    assert_eq!(manager.dismiss(default), Err("dictation_busy"));
    manager.finish(None, Ok(()), &CancellationToken::new());
    assert_eq!(manager.outcome(default), Some(Ok(())));
    assert_eq!(manager.installations().len(), 1);
    assert_eq!(manager.installations()[0].request, custom);
    manager.finish(Some(id), Ok(()), &CancellationToken::new());
    assert_eq!(manager.outcome(custom), Some(Ok(())));
    assert!(manager.installations().is_empty());
    assert!(manager.entries()[0].record.installed);
}
