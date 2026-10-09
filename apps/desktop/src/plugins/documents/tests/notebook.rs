use super::*;

fn value() -> Value {
    json!({"nbformat":4,"nbformat_minor":5,"metadata":{"kernelspec":{"language":"python"},"custom":"retained"},"cells":(0..21).map(|index| json!({"cell_type":"code","id":format!("cell-{index}"),"metadata":{"tags":["kept"]},"source":format!("print({index})"),"execution_count":index,"outputs":[{"output_type":"stream","name":"stdout","text":format!("{index}\n")}]})).collect::<Vec<_>>()})
}

#[gpui_kit::test]
fn previews_cells_and_preserves_source_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let path = fixture.directory.path().join("project/sample.ipynb");
        let original = value();
        std::fs::write(&path, original.to_string()).unwrap();
        let (panel, visual) = publication::mount(&fixture, cx);
        click(visual, "resource-file-sample.ipynb");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        wait(visual, |cx| {
            controller
                .read(cx)
                .documents
                .values()
                .any(|document| document.path == "sample.ipynb")
        });
        let id = controller.read_with(visual, |controller, _| {
            *controller.documents.keys().next().unwrap()
        });
        assert!(controller.read_with(visual, |controller, cx| {
            let document = &controller.documents[&id];
            document.notebook.as_ref().unwrap().preview
                && document.snapshot(id, cx)["mode"] == "document"
                && document.snapshot(id, cx)["can_edit"] == false
        }));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual
            .debug_bounds("notebook-preview")
            .expect("native Notebook preview must be rendered");
        let viewport = visual.update(|window, _| window.viewport_size());
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        assert!(bounds.intersects(&Bounds::new(point(px(0.), px(0.)), viewport)));
        click(visual, "notebook-more");
        wait(visual, |cx| {
            controller.read(cx).documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .next
                .is_none()
        });
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .count()),
            21
        );
        click(visual, "document-focus");
        visual.simulate_input("Cannot execute or edit this cell");
        assert!(
            !controller.read_with(visual, |controller, cx| controller.documents[&id].dirty(cx))
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            original
        );

        click(visual, "document-source");
        wait(visual, |cx| {
            !controller.read(cx).documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .preview
        });
        let mut draft = original.clone();
        draft["cells"][0]["source"] = json!("print(42)");
        publication::replace(&panel, visual, &draft.to_string());
        click(visual, "document-preview");
        wait(visual, |cx| {
            controller.read(cx).documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .preview
        });
        assert_eq!(
            controller.read_with(visual, |controller, cx| controller.documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .source(0, cx)),
            "print(42)"
        );
        assert!(controller.read_with(visual, |controller, cx| controller.documents[&id].dirty(cx)));
        click(visual, "document-save");
        wait(visual, |cx| !controller.read(cx).documents[&id].unsaved(cx));
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            draft
        );
        let Output::FileWritten(written) = fixture.execute(Command::WriteFile {
            worktree: fixture.binding.worktree.unwrap(),
            path: "sample.ipynb".into(),
            text: original.to_string(),
            expected_revision: controller.read_with(visual, |controller, _| {
                controller.documents[&id].revision.clone()
            }),
        }) else {
            panic!("written notebook expected")
        };
        visual.update(|window, cx| {
            controller.update(cx, |controller, cx| {
                controller.restored("sample.ipynb", false, window, cx)
            })
        });
        wait(visual, |cx| {
            controller.read(cx).documents[&id].revision.as_ref() == Some(&written.revision)
        });
        assert_eq!(
            controller.read_with(visual, |controller, cx| controller.documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .source(0, cx)),
            "print(0)"
        );
        click(visual, "document-close");
        wait(visual, |cx| controller.read(cx).documents.is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        fixture.close();
    }
}

#[gpui_kit::test]
fn refuses_changed_continuations_without_replacing_buffers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let path = fixture.directory.path().join("project/sample.ipynb");
        let original = value().to_string();
        std::fs::write(&path, &original).unwrap();
        let (panel, visual) = publication::mount(&fixture, cx);
        click(visual, "resource-file-sample.ipynb");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        wait(visual, |cx| {
            controller
                .read(cx)
                .documents
                .values()
                .any(|document| document.path == "sample.ipynb")
        });
        let id = controller.read_with(visual, |controller, _| {
            *controller.documents.keys().next().unwrap()
        });
        let mut changed = value();
        changed["cells"][20]["source"] = json!("External change");
        std::fs::write(&path, changed.to_string()).unwrap();
        click(visual, "notebook-more");
        wait(visual, |cx| {
            !controller.read(cx).documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .loading
        });
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.documents[&id]
                .notebook
                .as_ref()
                .unwrap()
                .count()),
            20
        );
        assert_eq!(
            controller.read_with(visual, |controller, cx| controller.documents[&id]
                .input
                .read(cx)
                .value()
                .to_string()),
            original
        );
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            "notebook changed while reading cells"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        fixture.close();
    }
}
