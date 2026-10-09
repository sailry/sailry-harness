use super::*;

#[tokio::test]
async fn reads_branches_and_release_commits() {
    let server = Server::start(|request, _| {
        assert_eq!(request.path, "/api/models/org/model/refs");
        Reply::json(json!({"branches":[{"name":"main","targetCommit":REVISION}],"tags":[{"name":"v1","targetCommit":OLDER}]}))
    }).await;
    let versions = huggingface::versions(&server.http(), "org/model", &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        versions,
        vec![
            Version {
                name: "main".into(),
                revision: REVISION.into()
            },
            Version {
                name: "v1".into(),
                revision: OLDER.into()
            },
        ]
    );
}

#[tokio::test]
async fn peels_advertised_release_tags() {
    let server = Server::start(|request, _| {
        assert_eq!(request.path, "/org/model.git/info/refs");
        assert_eq!(request.query["service"], "git-upload-pack");
        Reply::body(advertisement(&[
            ("refs/heads/master", REVISION),
            ("refs/tags/v1", NEWER),
            ("refs/tags/v1^{}", OLDER),
        ]))
    })
    .await;
    let versions = modelscope::versions(&server.http(), "org/model", &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        versions,
        vec![
            Version {
                name: "master".into(),
                revision: REVISION.into()
            },
            Version {
                name: "v1".into(),
                revision: OLDER.into()
            },
        ]
    );
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn labels_unresolved_versions_truthfully() {
    let catalog = Catalog::embedding();
    let server = Server::start(move |request, _| catalog.ms_reply(request)).await;
    let versions = modelscope::versions(&server.http(), "org/model", &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        versions,
        vec![
            Version {
                name: "master".into(),
                revision: "master".into()
            },
            Version {
                name: "v1".into(),
                revision: "v1".into()
            },
        ]
    );
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn rejects_malformed_ref_advertisements() {
    for bytes in [
        b"not a ref advertisement".to_vec(),
        b"ffffshort".to_vec(),
        advertisement(&[("refs/heads/master", "short")]),
        advertisement(&[("refs/tags/v1^{}", REVISION)]),
    ] {
        let result = super::super::refs::Refs::parse(&bytes);
        assert!(result.is_err());
    }
}
