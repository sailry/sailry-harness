use super::*;

#[tokio::test]
async fn persists_generic_image_history() {
    use base64::Engine;
    let bytes = image_bytes();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let payload = json!({"snapshot_id":"sdk-snapshot","elements":[],"window_bounds":{"x":20,"y":30,"width":100,"height":80}});
    let outcome = json!({"content":[
        {"type":"text","text":"Native window state"},
        {"type":"image","mimeType":"image/png","data":encoded,"annotations":{"audience":["assistant"]},"_meta":{"marker":7}}
    ],"structuredContent":payload,"isError":false,"_meta":{"native":true}});
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            vec![(
                "get_window_state".into(),
                json!({"pid":314,"window_id":271}),
            )],
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":outcome}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        let (result, images) = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::ToolResult {
                    name,
                    result,
                    images,
                    ..
                } if name == "get_window_state" => Some((result, images)),
                _ => None,
            })
            .expect("native capture result");
        let mut expected = outcome.clone();
        expected["content"][1]
            .as_object_mut()
            .unwrap()
            .remove("data");
        assert_eq!(
            result, &expected,
            "raw metadata must survive the generic image adaptation"
        );
        assert_eq!(images.len(), 1);
        let Output::AttachmentDownload(download) = fixture
            .client
            .execute(fixture.client.prepare(Command::DownloadImage {
                session: fixture.session,
                image: images[0].clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("image download expected")
        };
        let mut downloaded = Vec::new();
        fixture
            .client
            .download_attachment(
                &download,
                &mut downloaded,
                sailry_link::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(downloaded, bytes);
        {
            let requests = fixture.server.requests.lock().unwrap();
            let received: Vec<_> = requests.last().unwrap()["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|message| message["content"].as_array())
                .flatten()
                .filter_map(|part| part["image_url"]["url"].as_str())
                .collect();
            assert_eq!(
                received,
                vec![format!("data:image/png;base64,{encoded}").as_str()]
            );
        }
        let session = fixture.session;
        let Fixture {
            _directory: directory,
            node,
            controller,
            client,
            worker,
            ..
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        let calls = worker.calls().len();
        let node = Node::start_with_computer(
            directory.path().join("node"),
            NetworkScope::default(),
            Some(worker.configuration.clone()),
        )
        .await
        .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let recovered = client
            .read_conversation(session, None, 1)
            .await
            .unwrap()
            .page;
        assert_eq!(recovered.entries, page.entries);
        assert_eq!(
            worker.calls().len(),
            calls,
            "history reads must not dispatch the native capture again"
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn image_bytes() -> Vec<u8> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[31, 63, 127, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}
