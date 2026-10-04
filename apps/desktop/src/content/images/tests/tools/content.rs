use super::*;

#[gpui::test]
fn renders(cx: &mut TestAppContext) {
    use sailry_protocol::tool::{Content, Presentation};
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, &png(256, 128));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).image_history().calls.len() == 1
        });
        let (key, work, trigger, image) = view.update(visual, |view, cx| {
            // UI behavior uses delivered standard content and a real authenticated image.
            // The Node MCP test separately verifies production and history reconstruction.
            let history = view.image_history_mut();
            let call = &mut Arc::make_mut(&mut history.calls)[0];
            call.name = "plugin_report".into();
            call.presentation = Presentation::Content;
            call.resolved = None;
            let source = call.source.clone();
            let response = call.response.as_ref().unwrap().clone();
            let snapshot = Arc::make_mut(history.snapshot.as_mut().unwrap());
            let page = Arc::make_mut(&mut snapshot.page);
            let source = page.entries.iter_mut().find(|entry| entry.id == source.entry).unwrap();
            let Part::ToolCall { name, display, presentation, .. } = &mut source.parts[call.source.index] else { panic!("tool call expected") };
            *name = "plugin_report".into();
            *display = None;
            *presentation = Presentation::Content;
            let entry = page.entries.iter_mut().find(|entry| entry.id == response.entry).unwrap();
            let Part::ToolResult { name, result, images, .. } = &mut entry.parts[response.index] else { panic!("tool result expected") };
            *name = "plugin_report".into();
            let content: Content = serde_json::from_value(json!({"version":1,"blocks":[
                {"kind":"table","columns":["Name","State"],"rows":[["[literal](https://example.com)","Ready"]]},
                {"kind":"diff","path":"hello.txt","text":"@@ -1 +1 @@\n-before\n+after\n"},
                {"kind":"image","index":images[0].index}
            ]})).unwrap();
            *result = json!({"output":{"sailry_content":content,"retained":"original result"}});
            call.content = Some(content);
            let keys = (format!("{}-{}", call.turn, call.source.key()),
                format!("live-turn-work-{}", call.turn),
                format!("live-{}-tools-{}", call.turn, call.source.key()), images[0].attachment.id);
            cx.notify();
            keys
        });
        tap(visual, &work);
        tap(visual, &trigger);
        for selector in [
            format!("tool-content-{key}-0-cell-0-0"),
            format!("tool-content-{key}-1"),
            format!("image-card-{image}"),
        ] {
            assert!(
                visual
                    .debug_bounds(Box::leak(selector.into_boxed_str()))
                    .is_some()
            );
        }
        // Decoding changes the card aspect ratio and moves the following controls.
        let cache = visual.update(|_, cx| view.read(cx).image_cache());
        wait(visual, |cx| {
            matches!(
                cache
                    .read(cx)
                    .entries
                    .get(&Key::Published(image))
                    .and_then(|entry| entry.result.as_ref()),
                Some(Ok(_))
            )
        });
        let raw = Box::leak(format!("{key}-raw-content").into_boxed_str());
        assert!(visual.debug_bounds(raw).is_none());
        let details = Box::leak(format!("tool-content-details-{key}").into_boxed_str());
        assert!(visual.debug_bounds(details).is_none());
        tap(visual, &format!("live-tool-copy-{key}"));
        let copied = visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap());
        let copied: serde_json::Value = serde_json::from_str(&copied).unwrap();
        assert_eq!(copied["output"]["retained"], "original result");
        // Invalid content retains literal output and the authenticated image gallery.
        view.update(visual, |view, cx| {
            let history = view.image_history_mut();
            let call = &mut Arc::make_mut(&mut history.calls)[0];
            call.content = None;
            let response = call.response.as_ref().unwrap().clone();
            let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
            let entry = page
                .entries
                .iter_mut()
                .find(|entry| entry.id == response.entry)
                .unwrap();
            let Part::ToolResult { result, .. } = &mut entry.parts[response.index] else {
                panic!("tool result expected")
            };
            result["output"]["sailry_content"]["version"] = json!(2);
            cx.notify();
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(format!("{key}-output-content").into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(format!("image-card-{image}").into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(format!("tool-content-{key}-0").into_boxed_str()))
                .is_none()
        );
        assert!(visual.debug_bounds(details).is_none());
        assert!(visual.debug_bounds(raw).is_none());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
