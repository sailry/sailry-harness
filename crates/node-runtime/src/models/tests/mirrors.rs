use super::*;

#[test]
fn routes_metadata_and_assets_through_one_base() {
    let mirror = "https://mirror.example/cache/";
    let http = Http::new(Hub::HuggingFace, Some(mirror)).unwrap();
    assert_eq!(
        http.url(&["api", "models"]).as_str(),
        "https://mirror.example/cache/api/models"
    );
    assert_eq!(
        http.repository(&["api", "models"], "org/model", &["refs"])
            .as_str(),
        "https://mirror.example/cache/api/models/org/model/refs"
    );
    let file = File {
        source: source(Hub::HuggingFace, REVISION),
        path: "tokenizer.json".into(),
        bytes: 1,
        sha256: None,
        blob: None,
    };
    assert_eq!(
        http.file(&file).unwrap().as_str(),
        format!("https://mirror.example/cache/org/model/resolve/{REVISION}/tokenizer.json")
    );
    let model_scope = Http::new(Hub::ModelScope, Some(mirror)).unwrap();
    assert_eq!(
        model_scope.url(&["openapi", "v1", "models"]).host_str(),
        Some("modelscope.cn")
    );
    assert_eq!(
        model_scope
            .file(&File {
                source: source(Hub::ModelScope, REVISION),
                ..file
            })
            .unwrap()
            .host_str(),
        Some("modelscope.cn")
    );
    for invalid in [
        "http://mirror.example",
        "https://user@mirror.example",
        "https://mirror.example?key=1",
        "https://mirror.example/#fragment",
    ] {
        assert!(Http::new(Hub::HuggingFace, Some(invalid)).is_err());
    }
}

#[tokio::test]
async fn requests_search_versions_and_tokenizer_at_mirror() {
    let catalog = Catalog::embedding();
    let tokenizer = catalog.files["tokenizer.json"].body.clone();
    let server = Server::start(move |request, _| {
        assert!(request.path.starts_with("/cache/"));
        let mut routed = request.clone();
        routed.path = routed.path.strip_prefix("/cache").unwrap().into();
        match routed.path.as_str() {
            "/api/models" => Reply::json(json!([{"id":"org/model"}])),
            "/api/models/org/model/refs" => {
                Reply::json(json!({"branches":[{"name":"main","targetCommit":REVISION}],"tags":[]}))
            }
            _ => catalog.hf_reply(&routed),
        }
    })
    .await;
    let http = server.http_at("cache/");
    let cancel = CancellationToken::new();
    let page = huggingface::search(
        &http,
        &Search {
            hub: Hub::HuggingFace,
            kind: Kind::Embedding,
            query: "model".into(),
            cursor: None,
        },
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(page.items[0].repository, "org/model");
    let versions = huggingface::versions(&http, "org/model", &cancel)
        .await
        .unwrap();
    assert_eq!(versions[0].revision, REVISION);
    let definition = resolve::definition(
        &http,
        Kind::Embedding,
        &source(Hub::HuggingFace, REVISION),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(
        definition
            .assets
            .iter()
            .find(|asset| asset.name == "tokenizer.json")
            .unwrap()
            .sha256,
        checksum(&tokenizer)
    );
    assert_eq!(server.requests().len(), 6);
    assert!(server.requests().iter().any(
        |request| request.path == format!("/cache/org/model/resolve/{REVISION}/tokenizer.json")
    ));
}

#[tokio::test]
async fn cancels_tokenizer_reads() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| {
        if request.path.ends_with("/tokenizer.json") {
            Reply::hold()
        } else {
            catalog.hf_reply(request)
        }
    })
    .await;
    let http = server.http();
    let cancel = CancellationToken::new();
    let selected = source(Hub::HuggingFace, REVISION);
    let request = resolve::definition(&http, Kind::Embedding, &selected, &cancel);
    let cancellation = async {
        server.wait(3).await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(request, cancellation);
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(server.requests()[2].path.ends_with("/tokenizer.json"));
}
