use super::*;
use std::sync::{Arc, atomic::Ordering};

pub(super) fn mount<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    fixture.package();
    std::fs::write(
        fixture
            .directory
            .path()
            .join("project/package/dev.sailry.platform/desktop/main.js"),
        include_str!("view.js"),
    )
    .unwrap();
    let package = fixture.install(0);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let controller = Store::default()
            .get(fixture.binding.clone(), None, window, cx)
            .unwrap();
        let panel = cx.new(|cx| {
            Panel::standalone(fixture.binding.clone(), cx).with_documents(Some(controller))
        });
        owner = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1100.), px(750.)));
    wait(visual, |cx| {
        panel.read(cx).ready_for(&package.summary.reference(), cx)
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("document-status-ready")
    });
    (panel, visual)
}

pub(super) fn open(
    panel: &Entity<Panel>,
    visual: &mut VisualTestContext,
) -> (
    Entity<Controller>,
    RequestId,
    Entity<gpui_kit::component::input::EditorState>,
) {
    click(visual, "resource-file-notes.txt");
    let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
    wait(visual, |cx| {
        controller
            .read(cx)
            .documents
            .values()
            .any(|document| !document.dismissed)
    });
    let (id, input) = controller.read_with(visual, |controller, _| {
        controller
            .documents
            .iter()
            .find(|(_, document)| !document.dismissed)
            .map(|(id, document)| (*id, document.input.clone()))
            .unwrap()
    });
    (controller, id, input)
}

pub(super) fn replace(panel: &Entity<Panel>, visual: &mut VisualTestContext, value: &str) {
    click(visual, "document-focus");
    wait(visual, |_| true);
    visual.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(value.to_owned())));
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_keystrokes("secondary-v");
    wait(visual, |cx| snapshot(panel, cx).contains("document-dirty"));
}

#[gpui_kit::test]
fn preserves_unknown_publications_and_newer_input(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let observed = Arc::new(super::observed::Observed::new(fixture.transport.clone()));
        let _release = observed.release_on_drop();
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        let (panel, visual) = mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual);
        let path = fixture.directory.path().join("project/notes.txt");
        let published = "A complete streamed document 中文 🙂\n".repeat(7_000);
        replace(&panel, visual, &published);
        observed.mode.store(2, Ordering::SeqCst);
        click(visual, "document-save");
        wait(visual, |_| observed.completed.load(Ordering::SeqCst) == 1);
        click(visual, "document-close");
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert!(controller.read_with(visual, |controller, _| controller.documents[&id].saving));
        assert!(!controller.read_with(visual, |controller, _| controller.documents[&id].dismissed));
        click(visual, "document-close");
        crate::prompts::tests::answer(visual, "files_discard");
        wait(visual, |cx| controller.read(cx).documents[&id].dismissed);
        let (_, reopened, retained) = open(&panel, visual);
        assert_eq!(reopened, id);
        assert_eq!(retained.entity_id(), input.entity_id());
        drop(retained);
        replace(&panel, visual, "Newer text after dispatch\n");
        observed.mode.store(0, Ordering::SeqCst);
        observed.release.add_permits(1);
        wait(visual, |cx| !controller.read(cx).documents[&id].saving);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), published);
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "Newer text after dispatch\n"
        );
        assert!(controller.read_with(visual, |controller, cx| controller.documents[&id].dirty(cx)));
        click(visual, "document-save");
        wait(visual, |cx| !controller.read(cx).documents[&id].unsaved(cx));

        observed.mode.store(3, Ordering::SeqCst);
        let uncertain = "An uncertain publication\n".repeat(9_000);
        replace(&panel, visual, &uncertain);
        click(visual, "document-save");
        wait(visual, |cx| controller.read(cx).documents[&id].uncertain);
        let request = controller.read_with(visual, |controller, _| {
            controller.documents[&id]
                .request
                .as_ref()
                .unwrap()
                .request
                .clone()
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), uncertain);
        let count = observed.requests.lock().unwrap().len();
        // The native leaf applies the unknown-outcome lock without waiting for script redraws.
        click(visual, "document-focus");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("Must not replace the pending payload");
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            uncertain
        );
        click(visual, "document-close");
        let (_, detail) = crate::prompts::tests::wait(visual);
        assert!(detail.contains("notes.txt"));
        assert!(detail.contains(crate::tr("files_discard_description").as_ref()));
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert!(controller.read_with(visual, |controller, _| !controller.documents[&id].dismissed));
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            uncertain
        );
        click(visual, "document-close");
        crate::prompts::tests::answer(visual, "files_discard");
        wait(visual, |cx| controller.read(cx).documents[&id].dismissed);
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.documents[&id]
                .request
                .as_ref()
                .unwrap()
                .request
                .clone()),
            request
        );
        let (_, reopened, same_input) = open(&panel, visual);
        assert_eq!(reopened, id);
        assert_eq!(same_input.entity_id(), input.entity_id());
        observed.mode.store(0, Ordering::SeqCst);
        click(visual, "document-save");
        wait(visual, |cx| !controller.read(cx).documents[&id].unsaved(cx));
        assert_eq!(observed.requests.lock().unwrap().len(), count + 1);
        assert_eq!(observed.requests.lock().unwrap().last(), Some(&request));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), uncertain);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(same_input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn cancels_staging_and_preserves_drafts_during_refresh(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let observed = Arc::new(super::observed::Observed::new(fixture.transport.clone()));
        let _release = observed.release_on_drop();
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        let original = "A complete source document 资料\n".repeat(8_000);
        let path = fixture.directory.path().join("project/notes.txt");
        std::fs::write(&path, &original).unwrap();
        let (panel, visual) = mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual);
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            original
        );
        let start =
            4_500 * "A complete source document 资料\n".len() + "A complete source document ".len();
        let selection = start..start + "资料".len();
        visual.update(|_, cx| {
            input.update(cx, |input, cx| {
                input.set_selected_range(selection.clone(), cx)
            })
        });
        assert_eq!(
            input.read_with(visual, |input, _| input.selected_range()),
            selection
        );
        let refreshed = original.replace("source", "newest");
        std::fs::write(&path, &refreshed).unwrap();
        click(visual, "document-refresh");
        wait(visual, |cx| input.read(cx).value().as_ref() == refreshed);
        assert_eq!(
            input.read_with(visual, |input, _| input.selected_range()),
            selection
        );

        let opened = observed.opened.load(Ordering::SeqCst);
        observed.mode.store(1, Ordering::SeqCst);
        std::fs::write(&path, &original).unwrap();
        click(visual, "document-refresh");
        wait(visual, |_| observed.opened.load(Ordering::SeqCst) > opened);
        replace(&panel, visual, "Draft created during refresh\n");
        observed.mode.store(0, Ordering::SeqCst);
        observed.release.add_permits(1);
        wait(visual, |cx| controller.read(cx).reads.is_empty());
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "Draft created during refresh\n"
        );
        click(visual, "document-save");
        wait(visual, |cx| {
            controller.read(cx).documents[&id].error.is_some()
        });
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.documents[&id]
                .error
                .as_ref()
                .unwrap()
                .code),
            ErrorCode::RevisionConflict
        );
        click(visual, "document-close");
        crate::prompts::tests::answer(visual, "files_discard");
        wait(visual, |cx| {
            !controller.read(cx).documents.contains_key(&id)
        });
        let (_, id, input) = open(&panel, visual);

        observed.mode.store(1, Ordering::SeqCst);
        let opened = observed.opened.load(Ordering::SeqCst);
        replace(&panel, visual, &"Unpublished staged bytes\n".repeat(10_000));
        click(visual, "document-save");
        wait(visual, |_| observed.opened.load(Ordering::SeqCst) > opened);
        let submitted = observed
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::FinishFileUpload { .. }))
            .count();
        click(visual, "document-close");
        let (_, detail) = crate::prompts::tests::wait(visual);
        assert!(detail.contains(crate::tr("files_discard_description").as_ref()));
        crate::prompts::tests::answer(visual, "files_discard");
        wait(visual, |cx| {
            !controller.read(cx).documents.contains_key(&id)
        });
        observed.mode.store(0, Ordering::SeqCst);
        observed.release.add_permits(1);
        open(&panel, visual);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            observed
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(request.command, Command::FinishFileUpload { .. }))
                .count(),
            submitted
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn keeps_large_files_read_only(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (panel, visual) = mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual);
        let oversized =
            "line of oversized text\n".repeat(sailry_client::MAX_DOCUMENT_BYTES / 23 + 1);
        assert!(oversized.len() > sailry_client::MAX_DOCUMENT_BYTES);
        visual.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.set_value(oversized.clone(), window, cx)
            })
        });
        wait(visual, |cx| controller.read(cx).documents[&id].dirty(cx));
        let writes = || {
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| {
                    matches!(
                        request.command,
                        Command::WriteFile { .. }
                            | Command::UploadFile(_)
                            | Command::FinishFileUpload { .. }
                    )
                })
                .count()
        };
        let before = writes();
        click(visual, "document-save");
        wait(visual, |cx| {
            controller.read(cx).documents[&id].error.is_some()
        });
        assert_eq!(writes(), before);
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            oversized
        );
        assert!(controller.read_with(visual, |controller, cx| controller.documents[&id].dirty(cx)));
        visual.update(|window, cx| input.update(cx, |input, cx| input.set_value("", window, cx)));
        click(visual, "document-save");
        wait(visual, |cx| !controller.read(cx).documents[&id].unsaved(cx));
        let path = fixture.directory.path().join("project/notes.txt");
        assert!(std::fs::read(&path).unwrap().is_empty());
        click(visual, "document-discard");
        wait(visual, |cx| {
            !controller.read(cx).documents.contains_key(&id)
        });
        std::fs::write(&path, &oversized).unwrap();
        let (_, id, preview) = open(&panel, visual);
        assert!(controller.read_with(visual, |controller, _| controller.documents[&id].truncated));
        assert!(controller.read_with(visual, |controller, _| {
            controller.documents[&id].revision.is_none()
        }));
        let text = preview.read_with(visual, |input, _| input.value().to_string());
        assert!(text.len() <= sailry_protocol::MAX_FILE_BYTES);
        let before = writes();
        click(visual, "document-focus");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("Must remain read-only");
        click(visual, "document-save");
        wait(visual, |_| true);
        assert_eq!(
            preview.read_with(visual, |input, _| input.value().to_string()),
            text
        );
        assert_eq!(writes(), before);
        assert!(!controller.read_with(visual, |controller, cx| {
            controller.documents[&id].unsaved(cx)
        }));
        assert_eq!(std::fs::read_to_string(path).unwrap(), oversized);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(preview);
        fixture.close();
    }
}
