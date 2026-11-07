use super::*;

fn request(hub: Hub) -> Search {
    Search {
        hub,
        kind: Kind::Embedding,
        query: "small multilingual".into(),
        cursor: None,
    }
}

#[tokio::test]
async fn preserves_hub_cursor_and_query() {
    let server = Server::start(|request, base| {
        assert_eq!(request.path, "/api/models");
        assert_eq!(
            request.query.get("search").map(String::as_str),
            Some("small multilingual")
        );
        assert!(!request.query.contains_key("pipeline_tag"));
        assert_eq!(request.query.get("limit").map(String::as_str), Some("20"));
        let next = base
            .join("api/models?cursor=opaque%2B%2F%3D&limit=20")
            .unwrap();
        Reply::json(json!([{"id":"org/first"}])).header("Link", format!("<{next}>; rel=\"next\""))
    })
    .await;
    let http = server.http();
    let mut requested = request(Hub::HuggingFace);
    let page = huggingface::search(&http, &requested, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(page.items[0].repository, "org/first");
    assert_eq!(page.next_cursor.as_deref(), Some("opaque+/="));
    assert!(!page.partial);
    requested.cursor = page.next_cursor;
    huggingface::search(&http, &requested, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        server.requests()[1].query.get("cursor").map(String::as_str),
        Some("opaque+/=")
    );
}

#[tokio::test]
async fn refuses_foreign_pagination_endpoints() {
    let server = Server::start(|_, _| {
        Reply::json(json!([])).header(
            "Link",
            "<https://different.example/api/models?cursor=secret>; rel=\"next\"".into(),
        )
    })
    .await;
    let error = huggingface::search(
        &server.http(),
        &request(Hub::HuggingFace),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn retains_results_at_service_limit() {
    let server = Server::start(|request, _| {
        let page = request.query["page_number"].parse::<u32>().unwrap();
        assert_eq!(request.query["page_size"], "20");
        assert!(!request.query.contains_key("task"));
        assert!(!request.query.contains_key("filter.task"));
        Reply::json(json!({"success":true,"data":{
            "models":[{"id":"org/model","display_name":"Public model"}],
            "total_count":4000,"page_number":page,"page_size":20
        }}))
    })
    .await;
    let mut request = request(Hub::ModelScope);
    let page = modelscope::search(&server.http(), &request, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    assert!(!page.partial);
    request.cursor = Some("150".into());
    let page = modelscope::search(&server.http(), &request, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(page.items[0].name, "Public model");
    assert!(page.next_cursor.is_none());
    assert!(page.partial);
    request.cursor = Some("151".into());
    assert_eq!(
        modelscope::search(&server.http(), &request, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn rejects_changed_page_and_invalid_cursor() {
    let server = Server::start(|_, _| {
        Reply::json(json!({"success":true,"data":{
            "models":[],"total_count":0,"page_number":2,"page_size":20
        }}))
    })
    .await;
    let mut request = request(Hub::ModelScope);
    for cursor in ["0", "opaque", "4294967295"] {
        request.cursor = Some(cursor.into());
        assert_eq!(
            modelscope::search(&server.http(), &request, &CancellationToken::new())
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert!(server.requests().is_empty());
    request.cursor = None;
    assert_eq!(
        modelscope::search(&server.http(), &request, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn finds_exports_without_task_metadata() {
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        let server = Server::start(move |request, _| {
            if hub == Hub::HuggingFace {
                assert!(!request.query.contains_key("pipeline_tag"));
                Reply::json(json!([{"id":"org/sensevoice","pipeline_tag":null}]))
            } else {
                assert!(!request.query.contains_key("task"));
                assert!(!request.query.contains_key("filter.task"));
                Reply::json(json!({"success":true,"data":{"models":[{"id":"org/sensevoice","tasks":[]}],"total_count":1,"page_number":1,"page_size":20}}))
            }
        }).await;
        let mut request = request(hub);
        request.kind = Kind::Speech;
        let page = match hub {
            Hub::HuggingFace => {
                huggingface::search(&server.http(), &request, &CancellationToken::new())
                    .await
                    .unwrap()
            }
            Hub::ModelScope => {
                modelscope::search(&server.http(), &request, &CancellationToken::new())
                    .await
                    .unwrap()
            }
        };
        assert_eq!(page.items[0].repository, "org/sensevoice");
    }
}

#[tokio::test]
async fn bounds_declared_and_streamed_metadata() {
    for chunked in [false, true] {
        let server = Server::start(move |_, _| {
            if chunked {
                Reply::body(vec![b' '; METADATA_BYTES + 1]).chunked()
            } else {
                Reply::json(json!([])).oversized(METADATA_BYTES + 1)
            }
        })
        .await;
        let error = huggingface::search(
            &server.http(),
            &request(Hub::HuggingFace),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("size limit"));
    }
}

#[tokio::test]
async fn cancels_held_metadata_reads() {
    let server = Server::start(|_, _| Reply::hold()).await;
    let http = server.http();
    let cancel = CancellationToken::new();
    let stopping = cancel.clone();
    let task = tokio::spawn(async move {
        huggingface::search(&http, &request(Hub::HuggingFace), &stopping).await
    });
    server.wait(1).await;
    server.headers().await;
    cancel.cancel();
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn avoids_requests_after_cancellation() {
    let server = Server::start(|_, _| Reply::json(json!([]))).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let error = huggingface::search(&server.http(), &request(Hub::HuggingFace), &cancel)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn marks_filtered_invalid_metadata_partial() {
    let server = Server::start(|_, _| {
        Reply::json(json!([
            {"id":"org/valid"}, {"id":"../../other"}
        ]))
    })
    .await;
    let page = huggingface::search(
        &server.http(),
        &request(Hub::HuggingFace),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].repository, "org/valid");
    assert!(page.partial);
}
