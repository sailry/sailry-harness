use super::*;
use core::prelude::v1::test;
use gpui_kit::component::select::SelectEvent;
use sailry_protocol::{
    Command,
    knowledge::{Definition, Source},
};
pub(in crate::settings) mod fixture;
use fixture::{Fixture, draw, init, input, selected, tap, toast, wait};

#[gpui::test]
fn pauses_and_resumes_each_host_index(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.keyword();
        let path = fixture.source().join("notes.txt");
        let sailry_protocol::Output::KnowledgeCollection(collection) =
            fixture.execute(Command::CreateKnowledgeCollection {
                definition: Definition {
                    name: "Notes".into(),
                    source: Source::File {
                        path: path.to_str().unwrap().into(),
                    },
                    model: None,
                    globs: Vec::new(),
                    tags: Vec::new(),
                    shared_with: Vec::new(),
                },
                plugin: None,
            })
        else {
            panic!("collection expected")
        };
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, &format!("knowledge-pause-{}", collection.id));
        wait(visual, |cx| {
            panel
                .read(cx)
                .collections
                .iter()
                .any(|current| current.id == collection.id && current.paused)
                && !panel.read(cx).pending
        });
        let paused = fixture.collections().pop().unwrap();
        assert!(paused.paused);
        assert_eq!(paused.revision, collection.revision + 1);
        assert_eq!(paused.rebuild, collection.rebuild);
        toast(visual, "knowledge_pause_requested");
        tap(visual, &format!("knowledge-pause-{}", collection.id));
        wait(visual, |cx| {
            panel
                .read(cx)
                .collections
                .iter()
                .any(|current| current.id == collection.id && !current.paused)
                && !panel.read(cx).pending
        });
        let resumed = fixture.collections().pop().unwrap();
        assert!(!resumed.paused);
        assert_eq!(resumed.revision, paused.revision + 1);
        assert_eq!(resumed.rebuild, collection.rebuild);
        let requests = fixture.transport.requests.lock().unwrap();
        let toggles = requests
            .iter()
            .filter_map(|request| match request.command {
                Command::PauseKnowledgeCollection {
                    id,
                    expected_revision,
                    paused,
                } => Some((id, expected_revision, paused)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            toggles,
            [
                (collection.id, collection.revision, true),
                (collection.id, paused.revision, false)
            ]
        );
        drop(requests);
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "Knowledge fixture text\n"
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn saves_and_observes_selected_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        visual.update(|window, cx| {
            let model = panel.read(cx).model.clone();
            model.update(cx, |model, cx| {
                model.set_selected_value(&None, window, cx);
                cx.emit(SelectEvent::Confirm(model.selected_value().cloned()));
            });
        });
        input(visual, "knowledge-mirror", "https://mirror.example.test");
        tap(visual, "knowledge-settings-save");
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).saved.as_ref().unwrap().model.is_none()
        });
        assert_eq!(
            fixture.settings().mirror.as_deref(),
            Some("https://mirror.example.test")
        );
        assert_eq!(fixture.settings().revision, 1);
        fixture.bind(&owner, visual, true);
        assert_eq!(
            visual.update(|_, cx| selected(&owner, cx)
                .unwrap()
                .read(cx)
                .saved
                .clone()
                .unwrap()),
            Settings::default()
        );
        fixture.bind(&owner, visual, false);
        let mut settings = fixture.settings();
        settings.mirror = None;
        fixture.execute(Command::SaveKnowledgeSettings(settings));
        wait(visual, |cx| {
            panel
                .read(cx)
                .saved
                .as_ref()
                .is_some_and(|saved| saved.revision == 2 && saved.mirror.is_none())
        });
        fixture.close(visual);
    }
}

#[gpui::test]
fn creates_shared_folder_and_deletes_index(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.keyword();
        let source = fixture.source();
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-add");
        input(visual, "knowledge-name", "Notes");
        input(visual, "knowledge-path", source.to_str().unwrap());
        input(visual, "knowledge-globs", "*.txt");
        assert!(fixture.collections().is_empty());
        tap(visual, "knowledge-share");
        tap(visual, "knowledge-editor-save");
        fixture::wait_for(
            visual,
            |cx| {
                panel.read(cx).collections.len() == 1
                    && panel
                        .read(cx)
                        .editor
                        .as_ref()
                        .and_then(|editor| editor.upgrade())
                        .is_none_or(|editor| editor.read(cx).closed)
            },
            |window, cx| {
                use gpui_kit::test::TestWindowExt as _;
                let panel = panel.read(cx);
                let editor = panel
                    .editor
                    .as_ref()
                    .and_then(|editor| editor.upgrade())
                    .map(|editor| (editor.read(cx).pending, editor.read(cx).closed));
                let fields = [
                    "knowledge-name",
                    "knowledge-path",
                    "knowledge-globs",
                    "knowledge-share",
                ]
                .map(|id| (id, window.try_find(id)));
                format!(
                    "collection creation deadline (remote={remote}): loading={}, pending={}, error={:?}, collections={:?}, editor={editor:?}, draft={fields:?}, sent={:?}, outcomes={:?}",
                    panel.loading,
                    panel.pending,
                    panel.error,
                    panel.collections,
                    fixture.transport.requests.lock().unwrap(),
                    fixture.transport.outcomes.lock().unwrap(),
                )
            },
        );
        let collection = fixture.collections().pop().unwrap();
        assert_eq!(collection.definition.name, "Notes");
        assert_eq!(collection.definition.shared_with, ["knowledge"]);
        assert_eq!(collection.definition.globs, ["*.txt"]);
        assert_eq!(
            collection.definition.source,
            Source::Directory {
                path: source.to_string_lossy().into_owned()
            }
        );
        assert!(source.join("notes.txt").exists());
        let selector = format!("knowledge-delete-{}", collection.id);
        tap(visual, &selector);
        let (_, detail) = crate::prompts::tests::wait(visual);
        assert!(detail.contains("Notes"));
        assert!(detail.contains("source files are kept"));
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(fixture.collections().len(), 1);
        tap(visual, &selector);
        crate::prompts::tests::answer(visual, "knowledge_delete");
        wait(visual, |cx| panel.read(cx).collections.is_empty());
        assert!(fixture.collections().is_empty());
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "Knowledge fixture text\n"
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn creates_explicit_file_and_worktree_sources(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.keyword();
        let root = fixture.source();
        fixture.execute(Command::RegisterProject {
            name: "Knowledge fixture".into(),
            path: root.to_string_lossy().into_owned(),
        });
        let sailry_protocol::Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.path == root.to_string_lossy())
            .unwrap()
            .id;
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        for (kind, name, path, expected) in [
            (
                editor::SourceKind::File,
                "File source",
                root.join("notes.txt").to_string_lossy().into_owned(),
                Source::File {
                    path: root.join("notes.txt").to_string_lossy().into_owned(),
                },
            ),
            (
                editor::SourceKind::Worktree,
                "Worktree source",
                "notes.txt".into(),
                Source::Worktree {
                    worktree,
                    path: "notes.txt".into(),
                },
            ),
        ] {
            tap(visual, "knowledge-add");
            visual.update(|window, cx| {
                let editor = panel.read(cx).editor.as_ref().unwrap().upgrade().unwrap();
                let picker = editor.read(cx).source.clone();
                picker.update(cx, |picker, cx| {
                    picker.set_selected_value(&kind, window, cx);
                    cx.emit(SelectEvent::Confirm(picker.selected_value().copied()));
                });
            });
            input(visual, "knowledge-name", name);
            input(visual, "knowledge-path", &path);
            tap(visual, "knowledge-editor-save");
            wait(visual, |cx| {
                panel
                    .read(cx)
                    .collections
                    .iter()
                    .any(|collection| collection.definition.name == name)
                    && panel
                        .read(cx)
                        .editor
                        .as_ref()
                        .and_then(|editor| editor.upgrade())
                        .is_none_or(|editor| editor.read(cx).closed)
            });
            let collection = fixture
                .collections()
                .into_iter()
                .find(|collection| collection.definition.name == name)
                .unwrap();
            assert_eq!(collection.definition.source, expected);
            assert!(collection.definition.shared_with.is_empty());
        }
        fixture.close(visual);
    }
}

#[gpui::test]
fn cancels_while_install_completion_is_pending(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture
            .transport
            .hold_install
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-install-granite-multi");
        wait(visual, |cx| {
            panel.read(cx).pending
                && fixture
                    .transport
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| matches!(request.command, Command::InstallKnowledgeModel { .. }))
        });
        visual.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                let info = panel
                    .models
                    .iter_mut()
                    .find(|info| info.model == Model::GraniteMulti)
                    .unwrap();
                info.state = knowledge::ModelState::Downloading {
                    percent: 45,
                    bytes_per_second: 1024,
                };
                cx.notify();
            })
        });
        draw(visual);
        assert!(
            visual
                .debug_bounds("knowledge-cancel-granite-multi")
                .is_some()
        );
        tap(visual, "knowledge-cancel-granite-multi");
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).request.is_none()
                && panel.read(cx).cancelling.is_none()
        });
        toast(visual, "knowledge_cancel_requested");
        assert_eq!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(
                    request.command,
                    Command::CancelKnowledgeModel {
                        model: Model::GraniteMulti
                    }
                ))
                .count(),
            1
        );
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| request.target == fixture.node.id())
        );
        assert_eq!(visual.update(|_, cx| panel.read(cx).error), None);
        fixture.close(visual);
    }
}

#[gpui::test]
fn shares_library_progress_and_cancellation(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture
            .transport
            .hold_install
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-model-library");
        tap(visual, "model-library-knowledge-install-granite-multi");
        wait(visual, |cx| panel.read(cx).pending);
        visual.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel
                    .models
                    .iter_mut()
                    .find(|info| info.model == Model::GraniteMulti)
                    .unwrap()
                    .state = knowledge::ModelState::Downloading {
                    percent: 45,
                    bytes_per_second: 1024,
                };
                cx.notify();
            });
        });
        draw(visual);
        assert!(
            visual
                .debug_bounds("model-library-knowledge-progress-granite-multi")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("model-library-knowledge-speed-granite-multi")
                .is_some()
        );
        visual.update(|window, cx| window.close_dialog(cx));
        draw(visual);
        assert!(visual.update(|_, cx| panel.read(cx).pending));
        assert!(
            visual
                .debug_bounds("knowledge-progress-granite-multi")
                .is_some()
        );
        tap(visual, "knowledge-cancel-granite-multi");
        wait(visual, |cx| !panel.read(cx).pending);
        assert_eq!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(
                    request.command,
                    Command::CancelKnowledgeModel {
                        model: Model::GraniteMulti
                    }
                ))
                .count(),
            1
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_draft_and_explicit_shares(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.keyword();
        let source = fixture.source().join("notes.txt");
        let output = fixture.execute(Command::CreateKnowledgeCollection {
            definition: Definition {
                name: "Original".into(),
                source: Source::File {
                    path: source.to_string_lossy().into_owned(),
                },
                model: None,
                globs: vec![],
                tags: vec![],
                shared_with: vec!["files".into()],
            },
            plugin: None,
        });
        let sailry_protocol::Output::KnowledgeCollection(collection) = output else {
            panic!("collection expected")
        };
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, &format!("knowledge-edit-{}", collection.id));
        input(visual, "knowledge-name", "My draft");
        let mut definition = collection.definition.clone();
        definition.name = "Changed elsewhere".into();
        fixture.execute(Command::UpdateKnowledgeCollection {
            id: collection.id,
            expected_revision: collection.revision,
            definition,
        });
        tap(visual, "knowledge-editor-save");
        toast(visual, "knowledge_conflict");
        tap(visual, "knowledge-editor-retry");
        wait(visual, |cx| {
            panel
                .read(cx)
                .editor
                .as_ref()
                .and_then(|editor| editor.upgrade())
                .is_some_and(|editor| !editor.read(cx).pending)
        });
        tap(visual, "knowledge-editor-save");
        wait(visual, |cx| {
            panel
                .read(cx)
                .collections
                .iter()
                .any(|current| current.definition.name == "My draft")
        });
        let current = fixture.collections().pop().unwrap();
        assert_eq!(current.definition.shared_with, ["files"]);
        assert_eq!(current.definition.source, collection.definition.source);
        assert_eq!(current.revision, collection.revision + 2);
        fixture.close(visual);
    }
}

#[gpui::test]
fn retries_original_mutation_after_lost_completion(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.keyword();
        let source = fixture.source();
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        tap(visual, "knowledge-add");
        input(visual, "knowledge-name", "Once");
        input(visual, "knowledge-path", source.to_str().unwrap());
        fixture
            .transport
            .lose_reply
            .store(true, std::sync::atomic::Ordering::SeqCst);
        tap(visual, "knowledge-editor-save");
        toast(visual, "knowledge_unknown");
        assert_eq!(fixture.collections().len(), 1);
        tap(visual, "knowledge-editor-save");
        wait(visual, |cx| {
            panel
                .read(cx)
                .editor
                .as_ref()
                .and_then(|editor| editor.upgrade())
                .is_none_or(|editor| editor.read(cx).closed)
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let creates = requests
            .iter()
            .filter(|request| matches!(request.command, Command::CreateKnowledgeCollection { .. }))
            .collect::<Vec<_>>();
        assert_eq!(creates.len(), 2);
        assert_eq!(creates[0], creates[1]);
        assert_eq!(fixture.collections().len(), 1);
        drop(requests);
        fixture.close(visual);
    }
}

#[gpui::test]
fn rejects_editor_after_host_selection_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.keyword();
        let source = fixture.source();
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "knowledge-add");
        input(visual, "knowledge-name", "Old host draft");
        input(visual, "knowledge-path", source.to_str().unwrap());
        fixture.bind(&owner, visual, true);
        tap(visual, "knowledge-editor-save");
        toast(visual, "knowledge_target_changed");
        assert!(fixture.collections().is_empty());
        let client = sailry_client::Client::new(fixture.other.local());
        let output = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ListKnowledgeCollections)))
            .unwrap();
        assert!(
            matches!(output, sailry_protocol::Output::KnowledgeCollections(collections) if collections.is_empty())
        );
        tap(visual, "knowledge-editor-cancel");
        fixture.close(visual);
    }
}

#[gpui::test]
fn confirms_keyword_fallback_before_model_deletion(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    let (owner, visual) = fixture.mount(cx);
    let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
    visual.update(|_, cx| {
        panel.update(cx, |panel, cx| {
            panel
                .models
                .iter_mut()
                .find(|info| info.model == Model::GraniteMulti)
                .unwrap()
                .state = knowledge::ModelState::Installed;
            cx.notify();
        })
    });
    tap(visual, "knowledge-delete-model-granite-multi");
    let (_, detail) = crate::prompts::tests::wait(visual);
    assert!(detail.contains("Granite Multilingual"));
    assert!(detail.contains("fall back to keyword search"));
    crate::prompts::tests::answer(visual, "settings_cancel");
    assert!(visual.update(|_, cx| panel.read(cx).request.is_none()));
    fixture.close(visual);
}
