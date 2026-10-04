use super::*;
use base64::Engine;
use sailry_protocol::conversation::ModelApi;

use crate::native_provider_fixture as provider;

#[gpui::test]
fn sends_and_downloads_native_media(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let server = fixture.runtime.block_on(provider::Server::start(
            ModelApi::Gemini,
            provider::Reply::Text,
        ));
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.api = ModelApi::Gemini;
        provider.endpoint = server.endpoint.clone();
        provider.models[0].vision = true;
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let inputs: &[(&str, &str, &[u8])] = &[
            (
                "audio.bin",
                "audio/wav",
                include_bytes!("../../../../../../../tests/fixtures/audio.wav"),
            ),
            (
                "video.bin",
                "video/mp4",
                include_bytes!("../../../../../../../tests/fixtures/video.mp4"),
            ),
        ];
        let mut paths = Vec::new();
        for (name, _, bytes) in inputs {
            let path = fixture.directory.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            paths.push(path);
        }
        let (view, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        choose(visual, &paths);
        wait(visual, |cx| {
            view.read(cx).attachments.items.len() == 2 && view.read(cx).attachments.sendable()
        });
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            assert!(
                view.read(cx).error.is_none(),
                "send error: {:?}",
                view.read(cx).error
            );
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.runs.iter().any(|run| {
                        matches!(
                            run.status,
                            sailry_protocol::conversation::Status::Completed
                                | sailry_protocol::conversation::Status::Failed
                        )
                    })
                })
        });
        let attachments = view.read_with(visual, |view, _| {
            view.history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| {
                    if let Part::Attachment(attachment) = part {
                        Some(attachment.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(attachments.len(), 2);
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert!(
                page.runs
                    .iter()
                    .all(|run| run.status == sailry_protocol::conversation::Status::Completed),
                "runs: {:?}",
                page.runs
            );
        });
        let request = server.requests.lock().unwrap()[0].clone();
        for ((_, mime, bytes), attachment) in inputs.iter().zip(&attachments) {
            let native = request.body["contents"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|message| message["parts"].as_array().unwrap())
                .find(|part| part["inlineData"]["mimeType"] == *mime)
                .unwrap();
            assert_eq!(
                native["inlineData"]["data"],
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
            let destination = fixture
                .directory
                .path()
                .join(format!("download-{}", attachment.spec.name));
            download(visual, attachment, &destination);
            assert_eq!(std::fs::read(destination).unwrap(), *bytes);
        }
        view.read_with(visual, |view, _| {
            assert!(view.error.is_none());
            assert!(view.attachments.items.is_empty());
            let history = &view.history.snapshot.as_ref().unwrap().page;
            for attachment in attachments {
                assert!(
                    history
                        .entries
                        .iter()
                        .flat_map(|entry| &entry.parts)
                        .any(|part| *part == Part::Attachment(attachment.clone()))
                );
            }
        });
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
