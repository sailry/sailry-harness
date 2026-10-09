use super::*;
use core::prelude::v1::test;
use gpui_kit::TestAppContext;
use sailry_content::markdown::visuals::Kind;

fn request(source: &str) -> Request {
    Request {
        kind: Kind::Mermaid,
        source: source.into(),
        dark: false,
        foreground: "#000000".into(),
        background: "#ffffff".into(),
        accent: "#2468ab".into(),
        font_size: 14.0,
    }
}

#[test]
fn serves_only_fixed_local_assets() {
    let response = asset(
        &wry::http::Request::builder()
            .uri("sailry-visual://localhost/index.html")
            .body(Vec::new())
            .unwrap(),
    );
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["Content-Type"],
        "text/html; charset=utf-8"
    );
    for url in [
        "sailry-visual://localhost/../profile",
        "sailry-visual://localhost/index.html?url=https://example.com",
    ] {
        assert_eq!(
            asset(
                &wry::http::Request::builder()
                    .uri(url)
                    .body(Vec::new())
                    .unwrap()
            )
            .status(),
            404
        );
    }
    assert_eq!(
        asset(
            &wry::http::Request::builder()
                .method("POST")
                .uri(URL)
                .body(Vec::new())
                .unwrap()
        )
        .status(),
        404
    );
    assert!(navigation(URL.into()));
    assert!(navigation(
        "http://sailry-visual.localhost/index.html".into()
    ));
    for url in [
        "https://example.com",
        "file:///tmp/private",
        "sailry-visual://localhost/other",
    ] {
        assert!(!navigation(url.into()));
    }
}

#[test]
fn keeps_requested_source_inside_json() {
    let input = request("flowchart LR\nA[\"quote ' </script> \\\"\"] --> B");
    let rendered = script(7, &input);
    let prefix = "Promise.resolve().then(()=>window.sailryRender(";
    let end = rendered.find(")).then(").unwrap();
    let decoded: Request = serde_json::from_str(&rendered[prefix.len()..end]).unwrap();
    assert_eq!(decoded, input);
}

#[gpui_kit::test]
fn fails_waiters_when_bootstrap_breaks(cx: &mut TestAppContext) {
    let surface = cx.new(Surface::new);
    let (sender, mut receiver) = oneshot::channel();
    surface.update(cx, |surface, cx| {
        surface.generation = 2;
        surface.queue.push_back(Job {
            id: 1,
            request: request("flowchart LR\nA-->B"),
            sender,
        });
        surface.accept(
            Event::Result(
                1,
                Message {
                    id: 0,
                    result: None,
                    error: Some("stale".into()),
                },
            ),
            cx,
        );
        assert_eq!(surface.queue.len(), 1);
        surface.accept(
            Event::Result(
                2,
                Message {
                    id: 0,
                    result: None,
                    error: Some("Visual asset could not load".into()),
                },
            ),
            cx,
        );
        assert!(surface.queue.is_empty() && surface.active.is_none());
    });
    assert_eq!(
        receiver.try_recv().unwrap().unwrap_err(),
        "Visual asset could not load"
    );
}

#[gpui_kit::test]
fn ignores_stale_results_and_releases_waiters(cx: &mut TestAppContext) {
    let surface = cx.new(Surface::new);
    let (sender, mut receiver) = oneshot::channel();
    surface.update(cx, |surface, cx| {
        surface.generation = 2;
        surface.active = Some(Job {
            id: 9,
            request: request("flowchart LR\nA-->B"),
            sender,
        });
        surface.accept(
            Event::Result(
                1,
                Message {
                    id: 9,
                    result: None,
                    error: Some("stale".into()),
                },
            ),
            cx,
        );
        surface.accept(
            Event::Result(
                2,
                Message {
                    id: 8,
                    result: None,
                    error: Some("other".into()),
                },
            ),
            cx,
        );
        assert!(surface.active.is_some());
    });
    assert!(receiver.try_recv().is_err());
    surface.update(cx, |surface, _| surface.close("closed"));
    assert_eq!(receiver.try_recv().unwrap().unwrap_err(), "closed");
}
