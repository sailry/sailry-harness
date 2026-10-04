use super::*;
use sailry_link::Transport;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::{Part, Status as RunStatus};
use serde_json::{Value, json};

pub(super) fn fixture(remote: bool, bytes: &[u8]) -> Fixture {
    let mut fixture = Fixture::with_tools(
        remote,
        vec![(
            crate::agent_fixture::plugin_tool("files", "list_directory"),
            json!({"path":""}),
        )],
    );
    fixture.start();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 10,
        }) else {
            panic!("history expected")
        };
        if history
            .page
            .runs
            .iter()
            .any(|run| run.status == RunStatus::Completed)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "tool fixture completion deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let profile = fixture.node.profile().to_owned();
    fixture.runtime.block_on(fixture.node.shutdown()).unwrap();
    {
        // Only the isolated, stopped fixture is modified. Existing canonical
        // tool results must gain image projection without rewriting user history.
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let rows = {
            let mut query = db
                .prepare("SELECT sequence,body FROM agent_events ORDER BY sequence")
                .unwrap();
            query
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let mut inserted = false;
        for (sequence, body) in rows {
            let mut event: Value = serde_json::from_slice(&body).unwrap();
            let Some(parts) = event["content"]["parts"].as_array_mut() else {
                continue;
            };
            let Some(response) = parts
                .iter_mut()
                .find_map(|part| part.get_mut("functionResponse"))
            else {
                continue;
            };
            response["inline_data"] = json!([{"mime_type":"image/png","data":bytes}]);
            db.execute(
                "UPDATE agent_events SET body=?2 WHERE sequence=?1",
                rusqlite::params![sequence, serde_json::to_vec(&event).unwrap()],
            )
            .unwrap();
            inserted = true;
            break;
        }
        assert!(inserted, "fixture must contain a tool result");
    }
    fixture.node = fixture.runtime.block_on(Node::start(&profile)).unwrap();
    let transport: Arc<dyn Transport> = if remote {
        let address = fixture
            .runtime
            .block_on(
                fixture
                    .controller
                    .handle()
                    .pair(fixture.node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        fixture.controller.handle().remote(address)
    } else {
        fixture.node.local()
    };
    fixture.transport = transport.clone();
    let client = Arc::new(Client::new(transport));
    fixture.binding.client = client.clone();
    fixture.binding.defaults = client;
    fixture
}

#[gpui::test]
fn opens_persisted_images(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let bytes = png(256, 128);
        let fixture = fixture(remote, &bytes);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).image_history().calls.len() == 1
        });
        let (image, work, trigger, key) = view.read_with(visual, |view, _| {
            let page = &view.image_history().snapshot.as_ref().unwrap().page;
            let call = &view.image_history().calls[0];
            assert_eq!(call.presentation, sailry_protocol::tool::Presentation::Summary);
            assert!(matches!(page.entries.iter().flat_map(|entry| &entry.parts).find(|part| matches!(part, Part::ToolResult { .. })), Some(Part::ToolResult { images, .. }) if images.len() == 1));
            (
                call.images(page)[0].clone(),
                format!("live-turn-work-{}", call.turn),
                format!("live-{}-tools-{}", call.turn, call.source.key()),
                format!("{}-{}", call.turn, call.source.key()),
            )
        });
        let id = image.attachment.id;
        let card = Box::leak(format!("image-card-{id}").into_boxed_str());
        assert!(
            visual.debug_bounds(card).is_none(),
            "completed tool images stay collapsed until opened"
        );
        // Completed turns collapse their outer Work zone before tool details.
        tap(visual, &work);
        assert!(visual.debug_bounds(card).is_none());
        tap(visual, &trigger);
        let images = view.read_with(visual, |view, _| view.image_cache());
        wait(visual, |cx| {
            images
                .read(cx)
                .entries
                .get(&Key::Published(id))
                .is_some_and(|entry| matches!(entry.result, Some(Ok(_))))
        });
        let media = visual
            .debug_bounds(Box::leak(format!("image-media-{id}").into_boxed_str()))
            .unwrap();
        assert!(media.size.width > px(128.) && media.size.width <= px(320.));
        assert_eq!(media.size.width, media.size.height * 2.);
        for selector in [
            format!("{key}-output-content"),
            format!("live-tool-copy-{key}"),
            format!("live-tool-error-{key}"),
        ] {
            assert!(
                visual
                    .debug_bounds(Box::leak(selector.into_boxed_str()))
                    .is_none(),
                "successful tool images must not expose metadata or an empty result body"
            );
        }
        assert!(
            visual
                .debug_bounds(Box::leak(format!("attachment-open-{id}").into_boxed_str()))
                .is_none()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("attachment-download-{id}").into_boxed_str()
                ))
                .is_none()
        );
        tap(visual, card);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        tap(visual, "image-download");
        let destination = fixture.directory.path().join("tool-image.png");
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        fixture::finish_download(visual, &destination);
        assert_eq!(std::fs::read(destination).unwrap(), bytes);
        visual.simulate_keystrokes("escape");
        finish_close(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
        drop(images);
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn missing_metadata(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, &png(256, 128));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).image_history().calls.len() == 1
        });
        let (work, trigger, key, id) = view.read_with(visual, |view, _| {
            let page = &view.image_history().snapshot.as_ref().unwrap().page;
            let call = &view.image_history().calls[0];
            (
                format!("live-turn-work-{}", call.turn),
                format!("live-{}-tools-{}", call.turn, call.source.key()),
                format!("{}-{}", call.turn, call.source.key()),
                call.images(page)[0].attachment.id,
            )
        });
        tap(visual, &work);
        tap(visual, &trigger);
        let image = Box::leak(format!("image-card-{id}").into_boxed_str());
        let error = Box::leak(format!("live-tool-error-{key}").into_boxed_str());
        let output = Box::leak(format!("{key}-output-content").into_boxed_str());
        let copy = Box::leak(format!("live-tool-copy-{key}").into_boxed_str());
        for presentation in [
            sailry_protocol::tool::Presentation::Summary,
            sailry_protocol::tool::Presentation::Details,
        ] {
            for (name, result, failed) in [
                (
                    "get_window_state".to_owned(),
                    json!({"isError":false,"content":[{"type":"text","text":"Native window state"}],
                        "structuredContent":{"window_id":73,"pid":101,"element_count":24}}),
                    false,
                ),
                (
                    "click".to_owned(),
                    json!({"isError":true,"content":[{"type":"text","text":"Input failed"}],
                        "structuredContent":{"window_id":73,"error_code":"input_failed"}}),
                    true,
                ),
                (
                    "get_window_state".to_owned(),
                    json!({"isError":true,"content":[{"type":"text","text":"Window capture failed"}],
                        "structuredContent":{"pid":101,"error_code":"capture_failed"}}),
                    true,
                ),
                (
                    "plugin_capture".to_owned(),
                    json!({"window_id":73,"error":sailry_protocol::Fault::new(
                        sailry_protocol::ErrorCode::Unavailable,"capture unavailable"
                    )}),
                    true,
                ),
                (
                    "plugin_capture".to_owned(),
                    json!({"window_id":73,"pid":101,"element_count":24}),
                    false,
                ),
            ] {
                // Exercise delivered presentation only; no computer input is executed.
                view.update(visual, |view, cx| {
                    let call = &mut Arc::make_mut(&mut view.image_history_mut().calls)[0];
                    call.name = name;
                    call.presentation = presentation;
                    let response = call.response.as_ref().unwrap().clone();
                    let snapshot =
                        Arc::make_mut(view.image_history_mut().snapshot.as_mut().unwrap());
                    let page = Arc::make_mut(&mut snapshot.page);
                    let entry = page
                        .entries
                        .iter_mut()
                        .find(|entry| entry.id == response.entry)
                        .unwrap();
                    let Part::ToolResult { result: value, .. } = &mut entry.parts[response.index]
                    else {
                        panic!("tool result expected");
                    };
                    *value = result;
                    cx.notify();
                });
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(visual.debug_bounds(image).is_some());
                assert_eq!(visual.debug_bounds(error).is_some(), failed);
                assert!(visual.debug_bounds(output).is_none());
                assert!(visual.debug_bounds(copy).is_none());
            }
        }
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

mod content;
