use super::*;
use serde_json::json;

#[tokio::test]
async fn routes_to_requesting_controller() {
    let browsers = Arc::new(Browsers::default());
    let caller = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let mut calls = browsers.subscribe(caller).unwrap();
    let _other_calls = browsers.subscribe(other).unwrap();
    assert!(browsers.subscribe(caller).is_err());
    let session = SessionId::new();
    let id = RequestId::new();
    let owner = browsers.clone();
    let pending = tokio::spawn(async move { owner.call(caller, session, id, Action::Tabs).await });
    let Update::BrowserCall(call) = calls.next().await.unwrap() else {
        panic!("browser call expected")
    };
    assert_eq!(call.session, session);
    assert_eq!(call.id, id);
    assert_eq!(
        browsers
            .complete(other, id, Ok(json!({})))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    browsers
        .complete(caller, id, Ok(json!({"tabs":[]})))
        .unwrap();
    assert_eq!(pending.await.unwrap().unwrap(), json!({"tabs":[]}));
    assert!(browsers.complete(caller, id, Ok(json!({}))).is_err());
}

#[tokio::test]
async fn avoids_replay_after_disconnect() {
    let browsers = Arc::new(Browsers::default());
    let caller = NodeId([1; 32]);
    let mut calls = browsers.subscribe(caller).unwrap();
    let owner = browsers.clone();
    let pending = tokio::spawn(async move {
        owner
            .call(
                caller,
                SessionId::new(),
                RequestId::new(),
                Action::Back { tab: 0 },
            )
            .await
    });
    calls.next().await.unwrap();
    drop(calls);
    assert!(!browsers.available(caller));
    assert_eq!(
        pending.await.unwrap().unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    let mut calls = browsers.subscribe(caller).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), calls.next())
            .await
            .is_err()
    );
}

#[test]
fn bounds_capture_payloads() {
    let image = |bytes: usize| "A".repeat(bytes.div_ceil(3) * 4);
    let text = |bytes: usize| "t".repeat(bytes);
    assert!(bounded(&Ok(
        json!({"base64_image":image(browser::MAX_IMAGE_BYTES)})
    )));
    assert!(!bounded(&Ok(
        json!({"base64_image":image(browser::MAX_IMAGE_BYTES + 3)})
    )));
    assert!(bounded(&Ok(json!({
        "base64_image":image(browser::MAX_IMAGE_BYTES),
        "text":text(browser::MAX_RESULT_BYTES - 64)
    }))));
    assert!(!bounded(&Ok(json!({
        "base64_image":image(16),
        "text":text(browser::MAX_RESULT_BYTES)
    }))));
    assert!(!bounded(&Ok(
        json!({"text":text(browser::MAX_RESULT_BYTES)})
    )));
    assert!(bounded(&Err(Fault::invalid("browser capture failed"))));
}

mod capture {
    use super::*;
    use base64::Engine;
    use execute::Capture;
    use sailry_protocol::browser::Format;

    fn encoded(width: u32, height: u32, format: image::ImageFormat) -> String {
        let pixels = image::RgbImage::from_pixel(width, height, image::Rgb([32, 96, 160]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut bytes, format).unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    }

    #[test]
    fn publishes_inline_media_and_facts() {
        let data = encoded(3, 2, image::ImageFormat::Png);
        let capture = Capture::decode(
            json!({
                "tab":4,"base64_image":data,"bounds":{"x":1,"y":2,"width":3,"height":2},
                "path":"../escaped.png","inline_data":[{"mime_type":"image/png","data":"forged"}],
                "images":["sailry-image:forged/0"]
            }),
            Format::Png,
        )
        .unwrap();
        assert_eq!(
            capture.publish(None),
            json!({
                "tab":4,"bounds":{"x":1,"y":2,"width":3,"height":2},
                "mime_type":"image/png","width":3,"height":2,
                "inline_data":[{"mime_type":"image/png","data":data}]
            })
        );
        let data = encoded(2, 2, image::ImageFormat::Jpeg);
        let saved = Capture::decode(json!({"base64_image":data}), Format::Jpeg)
            .unwrap()
            .publish(Some("assets/generated/browser.jpg".into()));
        assert_eq!(saved["path"], "assets/generated/browser.jpg");
        assert_eq!(saved["inline_data"][0]["mime_type"], "image/jpeg");
    }

    #[test]
    fn rejects_unrequested_or_unbounded_images() {
        for (value, format) in [
            (json!({"tab":1}), Format::Jpeg),
            (json!({"base64_image":"not base64"}), Format::Jpeg),
            (
                json!({"base64_image":encoded(2, 2, image::ImageFormat::Png)}),
                Format::Jpeg,
            ),
            (
                json!({"base64_image":encoded(browser::MAX_IMAGE_SIDE + 1, 1, image::ImageFormat::Png)}),
                Format::Png,
            ),
            (json!(["base64_image"]), Format::Png),
        ] {
            assert!(Capture::decode(value, format).is_err());
        }
    }
}

#[tokio::test]
async fn removes_cancelled_completion() {
    let browsers = Arc::new(Browsers::default());
    let caller = NodeId([1; 32]);
    let mut calls = browsers.subscribe(caller).unwrap();
    let owner = browsers.clone();
    let id = RequestId::new();
    let pending =
        tokio::spawn(async move { owner.call(caller, SessionId::new(), id, Action::Tabs).await });
    calls.next().await.unwrap();
    pending.abort();
    let _ = pending.await;
    assert_eq!(
        browsers
            .complete(caller, id, Ok(json!({})))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
