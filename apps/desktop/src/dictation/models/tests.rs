use super::*;
use sailry_protocol::models::{Asset, Hub, Source};

fn definition() -> Definition {
    let source = Source {
        hub: Hub::HuggingFace,
        repository: "fixture/sensevoice".into(),
        revision: "a".repeat(40),
    };
    Definition {
        source: source.clone(),
        name: "Fixture speech".into(),
        license: "Apache-2.0".into(),
        query_prefix: None,
        document_prefix: None,
        format: Format::SenseVoice {},
        assets: ["model.onnx", "tokens.txt"]
            .into_iter()
            .map(|name| Asset {
                source: source.clone(),
                path: name.into(),
                name: name.into(),
                bytes: 4,
                sha256: "b".repeat(64),
            })
            .collect(),
    }
}

pub(in crate::dictation) fn installed(directory: PathBuf) -> (Arc<Manager>, ModelId, Capture) {
    let manager = Manager::open(directory).unwrap();
    let id = manager.register(definition()).unwrap();
    manager.finish(Some(id), Ok(()), &CancellationToken::new());
    let captured = manager.capture(Some(id)).unwrap();
    (manager, id, captured)
}

#[test]
fn preserves_installed_selection_records() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let definition = definition();
    let id = manager.register(definition.clone()).unwrap();
    assert_eq!(manager.register(definition.clone()).unwrap(), id);
    manager.finish(Some(id), Ok(()), &CancellationToken::new());
    let reopened = Manager::open(directory.path().into()).unwrap();
    let entry = reopened.entries().pop().unwrap();
    assert_eq!(entry.record.id, id);
    assert_eq!(entry.record.definition, definition);
    assert_eq!(entry.state, State::Installed);
    assert!(!reopened.capture(Some(id)).unwrap().ready());
}

#[test]
fn immutable_versions_reject_new_assets() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let mut definition = definition();
    manager.register(definition.clone()).unwrap();
    definition.assets[0].sha256 = "c".repeat(64);
    assert_eq!(manager.register(definition), Err("model_library_conflict"));
    assert_eq!(manager.entries().len(), 1);
}

#[test]
fn captured_model_keeps_its_installation() {
    let directory = tempfile::tempdir().unwrap();
    let (manager, id, captured) = installed(directory.path().into());
    let cloned = captured.clone();
    assert_eq!(manager.remove(id), Err("dictation_busy"));
    let mut other = definition();
    other.source.revision = "d".repeat(40);
    let other = manager.register(other).unwrap();
    manager.remove(other).unwrap();
    drop(captured);
    assert_eq!(manager.remove(id), Err("dictation_busy"));
    drop(cloned);
    manager.remove(id).unwrap();
    assert!(manager.entries().is_empty());
}

#[test]
fn cancellation_never_publishes_an_installation() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let id = manager.register(definition()).unwrap();
    let cancel = CancellationToken::new();
    manager.data.lock().unwrap().jobs.insert(
        Some(id),
        Job {
            cancel: cancel.clone(),
            state: State::Downloading(Progress::default()),
        },
    );
    manager.cancel(Some(id));
    assert!(cancel.is_cancelled());
    manager.finish(Some(id), Ok(()), &cancel);
    assert_eq!(manager.entries()[0].state, State::Missing);
    assert!(
        !Manager::open(directory.path().into()).unwrap().entries()[0]
            .record
            .installed
    );
}

#[test]
fn preserves_invalid_index() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("models")).unwrap();
    let path = directory.path().join("models/speech.json");
    std::fs::write(&path, b"invalid").unwrap();
    assert!(Manager::open(directory.path().into()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"invalid");
}

#[test]
fn cancels_only_the_addressed_model() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let id = manager.register(definition()).unwrap();
    let default = CancellationToken::new();
    let custom = CancellationToken::new();
    {
        let mut data = manager.data.lock().unwrap();
        for (id, cancel) in [(None, default.clone()), (Some(id), custom.clone())] {
            data.jobs.insert(
                id,
                Job {
                    cancel,
                    state: State::Downloading(Progress::default()),
                },
            );
        }
    }
    manager.cancel(None);
    assert!(default.is_cancelled());
    assert!(!custom.is_cancelled());
    assert!(manager.cancelling(None));
    assert!(!manager.cancelling(Some(id)));
    manager.cancel(Some(id));
    assert!(custom.is_cancelled());
}

#[test]
fn restores_files_when_index_save_fails() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let id = manager.register(definition()).unwrap();
    manager.finish(Some(id), Ok(()), &CancellationToken::new());
    let model = manager.path(Some(id));
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(model.join("model.onnx"), b"original graph").unwrap();
    let index = directory.path().join("models/speech.json");
    std::fs::remove_file(&index).unwrap();
    std::fs::create_dir(&index).unwrap();
    assert_eq!(manager.remove(id), Err("dictation_storage"));
    assert_eq!(manager.entries()[0].record.id, id);
    assert!(manager.entries()[0].record.installed);
    assert_eq!(
        std::fs::read(model.join("model.onnx")).unwrap(),
        b"original graph"
    );
    assert_eq!(
        std::fs::read_dir(model.parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn preserves_installation_when_resaving_fails() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let id = manager.register(definition()).unwrap();
    manager.finish(Some(id), Ok(()), &CancellationToken::new());
    let index = directory.path().join("models/speech.json");
    std::fs::remove_file(&index).unwrap();
    std::fs::create_dir(&index).unwrap();
    manager.data.lock().unwrap().jobs.insert(
        Some(id),
        Job {
            cancel: CancellationToken::new(),
            state: State::Downloading(Progress::default()),
        },
    );
    manager.finish(Some(id), Ok(()), &CancellationToken::new());
    let entry = manager.entries().pop().unwrap();
    assert!(entry.record.installed);
    assert_eq!(entry.state, State::Failed("dictation_storage"));
    assert!(manager.capture(Some(id)).is_ok());
}

#[test]
fn checking_preserves_an_active_download() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Manager::open(directory.path().into()).unwrap();
    let progress = Progress {
        percent: 45.,
        bytes_per_second: 1024.,
    };
    manager.data.lock().unwrap().jobs.insert(
        None,
        Job {
            cancel: CancellationToken::new(),
            state: State::Downloading(progress),
        },
    );
    manager.observe(None, false);
    assert_eq!(manager.default_state(), Some(State::Downloading(progress)));
    manager.finish(None, Ok(()), &CancellationToken::new());
    manager.observe(None, true);
    assert_eq!(manager.default_state(), Some(State::Installed));
}
