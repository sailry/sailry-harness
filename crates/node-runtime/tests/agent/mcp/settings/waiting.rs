use super::*;

#[tokio::test]
async fn preserves_bound_configuration() {
    let model = Server::turn_tools(vec![(
        alias("native", "write"),
        json!({"value":"approved"}),
    )])
    .await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    declare(&fixture.root);
    let installed = install(&fixture, 0).await;
    let secret = "bound-private-token";
    let configured = save(
        &fixture,
        installed.summary.reference(),
        "Original",
        SecretUpdate::Replace(Secret::new(secret.into())),
    )
    .await;
    let data = fixture.node.profile().join("plugins/data/example");
    super::super::sharing::warmed(&data, 1).await;
    assert!(model.requests.lock().unwrap().is_empty());
    let (_, turn) = submit(&fixture, false).await;
    let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(approval.turn, turn.id);
    assert_eq!(super::super::connections::requests(&data, "initialize"), 1);
    let updated = save(&fixture, configured.package, "Updated", SecretUpdate::Keep).await;
    super::super::sharing::warmed(&data, 2).await;
    assert_eq!(super::super::connections::requests(&data, "tools/call"), 0);
    execute(
        &fixture.client,
        Command::ResolveApproval {
            session: fixture.session.id,
            approval: approval.id,
            decision: Decision::Approve,
        },
    )
    .await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    let output = results(&fixture.client, &page).await;
    let path = output.last().unwrap()["structuredContent"]["settings_path"]
        .as_str()
        .expect("frozen settings projection expected");
    tokio::time::timeout(Duration::from_secs(10), async {
        while Path::new(path).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("retired projection cleanup deadline");
    verify(&fixture.client, &page, "Original", secret, false).await;
    let (_, next) = submit(&fixture, false).await;
    assert!(next.plugins.contains(&updated.package));
    let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(approval.turn, next.id);
    execute(
        &fixture.client,
        Command::ResolveApproval {
            session: fixture.session.id,
            approval: approval.id,
            decision: Decision::Approve,
        },
    )
    .await;
    let page = finished(&fixture.client, fixture.session.id, next.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    verify(&fixture.client, &page, "Updated", secret, true).await;
    assert_eq!(super::super::connections::requests(&data, "initialize"), 2);
    assert_eq!(super::super::connections::requests(&data, "tools/call"), 2);
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}
