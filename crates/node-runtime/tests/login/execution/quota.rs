use super::*;
use sailry_client::{Apply, Projection};
use sailry_protocol::conversation::quota::Snapshot as Quota;

fn response(request: &discovery_support::Request, values: &[(&str, String)]) -> Reply {
    let Reply::Raw(raw) = generation(request, Authentication::ChatGpt, false) else {
        unreachable!()
    };
    let headers = values
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    Reply::Raw(raw.replacen("Content-Type:", &format!("{headers}Content-Type:"), 1))
}

fn known(request: &discovery_support::Request) -> Reply {
    response(
        request,
        &[
            ("x-codex-primary-window-minutes", "10080".into()),
            ("x-codex-primary-used-percent", "100.5".into()),
            ("x-codex-primary-reset-at", future_expiry().to_string()),
            ("x-codex-secondary-window-minutes", "300".into()),
            ("x-codex-secondary-used-percent", "40.5".into()),
        ],
    )
}

async fn read(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    no_secrets(&snapshot);
    snapshot
}

async fn credential(client: &Client, reference: &CredentialRef) -> Credential {
    let Output::Credentials(credentials) = client
        .execute(client.prepare(Command::ListCredentials))
        .await
        .unwrap()
    else {
        panic!("credentials expected")
    };
    credentials
        .into_iter()
        .find(|credential| credential.id == reference.id)
        .unwrap()
}

async fn changed(
    subscription: &mut Box<dyn Subscription>,
    projection: &mut Projection,
    provider: ProviderId,
    present: bool,
) -> Option<Quota> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let update = subscription.next().await.unwrap();
            let covered = matches!(&update, Update::Event(envelope)
                if envelope.cursor <= projection.snapshot().unwrap().cursor);
            let observed = match &update {
                Update::Event(envelope) => match &envelope.event {
                    Event::ProviderQuotaChanged {
                        provider: id,
                        quota,
                    } if *id == provider && quota.is_some() == present => Some(quota.clone()),
                    _ => None,
                },
                _ => None,
            };
            assert_eq!(
                projection.apply(0, update).unwrap(),
                if covered {
                    Apply::Ignored
                } else {
                    Apply::Applied
                }
            );
            if let Some(quota) = observed
                && !covered
            {
                return quota;
            }
        }
    })
    .await
    .expect("quota observation deadline")
}

fn expire(fixture: &Fixture, provider: &Provider) {
    let database =
        rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3")).unwrap();
    let mut body: serde_json::Value = serde_json::from_str(
        &database
            .query_row::<String, _, _>(
                "SELECT CAST(body AS TEXT) FROM providers WHERE id=?1",
                [provider.id.to_string()],
                |row| row.get(0),
            )
            .unwrap(),
    )
    .unwrap();
    let id = provider.credential.as_ref().unwrap().id.to_string();
    let saved = &mut body["authorizations"][id];
    let mut grant: serde_json::Value =
        serde_json::from_str(saved["key"].as_str().unwrap()).unwrap();
    grant["expires_at_ms"] = json!((future_expiry() - 3601) * 1000);
    saved["key"] = json!(grant.to_string());
    database
        .execute(
            "UPDATE providers SET body=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_vec(&body).unwrap(), provider.id.to_string()],
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refreshes_bound_observations() {
    for remote in [false, true] {
        let exchanges = Arc::new(AtomicUsize::new(0));
        let count = exchanges.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let ready = release.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if exchange(request) {
                let grant = exchanged(request, Authentication::ChatGpt, true);
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    Reply::Json(grant)
                } else {
                    Reply::Delayed(ready.clone(), grant)
                }
            } else if request.path == "/responses" {
                known(request)
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let session = session(&fixture, &provider).await;
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 0);
        projection
            .apply(0, subscription.next().await.unwrap())
            .unwrap();
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Completed).await;
        let original = changed(&mut subscription, &mut projection, provider.id, true)
            .await
            .unwrap();
        assert_eq!(original.credential, provider.credential.clone().unwrap());
        assert_eq!(
            original.revision,
            credential(&fixture.client, &original.credential)
                .await
                .revision
        );
        assert!(original.observed_ms > 0);
        assert_eq!(original.five_hour.as_ref().unwrap().used_basis_points, 4050);
        assert_eq!(original.five_hour.as_ref().unwrap().reset_at_ms, None);
        assert_eq!(
            original.seven_day.as_ref().unwrap().used_basis_points,
            10050
        );
        assert_eq!(
            original
                .seven_day
                .as_ref()
                .unwrap()
                .remaining_basis_points(),
            0
        );
        assert!(
            serde_json::to_value(&original)
                .unwrap()
                .get("plan")
                .is_none()
        );
        assert_eq!(
            projection.snapshot().unwrap().provider_quotas,
            vec![original.clone()]
        );
        assert_eq!(
            read(&fixture.client).await.provider_quotas,
            vec![original.clone()]
        );
        drop(subscription);
        expire(&fixture, &provider);
        let fixture = renewals::restart(fixture, &server, remote).await;
        assert_eq!(
            read(&fixture.client).await.provider_quotas,
            vec![original.clone()]
        );
        assert_eq!(
            exchanges.load(Ordering::SeqCst),
            1,
            "restoring quota must not refresh or poll"
        );
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 0);
        projection
            .apply(0, subscription.next().await.unwrap())
            .unwrap();
        let (_, turn) = submit(&fixture.client, &session).await;
        assert_eq!(
            changed(&mut subscription, &mut projection, provider.id, false).await,
            None
        );
        assert!(read(&fixture.client).await.provider_quotas.is_empty());
        assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
        release.notify_one();
        finished(&fixture.client, &turn, Status::Completed).await;
        let refreshed = changed(&mut subscription, &mut projection, provider.id, true)
            .await
            .unwrap();
        assert_eq!(refreshed.credential, original.credential);
        assert!(refreshed.revision > original.revision);
        assert_eq!(
            refreshed.revision,
            credential(&fixture.client, &refreshed.credential)
                .await
                .revision
        );
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        drop(subscription);
        let fixture = renewals::restart(fixture, &server, remote).await;
        assert_eq!(read(&fixture.client).await.provider_quotas, vec![refreshed]);
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clears_missing_response_headers() {
    for remote in [false, true] {
        let generations = AtomicUsize::new(0);
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/responses" {
                if generations.fetch_add(1, Ordering::SeqCst) == 0 {
                    known(request)
                } else {
                    response(request, &[])
                }
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let session = session(&fixture, &provider).await;
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Completed).await;
        assert_eq!(read(&fixture.client).await.provider_quotas.len(), 1);
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 0);
        projection
            .apply(0, subscription.next().await.unwrap())
            .unwrap();
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Completed).await;
        assert_eq!(
            changed(&mut subscription, &mut projection, provider.id, false).await,
            None
        );
        assert!(read(&fixture.client).await.provider_quotas.is_empty());
        drop(subscription);
        let requests = server.requests.lock().unwrap().len();
        let fixture = renewals::restart(fixture, &server, remote).await;
        assert!(read(&fixture.client).await.provider_quotas.is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), requests);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clears_revoked_credentials() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |request| {
            if request.path == "/responses" {
                known(request)
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let session = session(&fixture, &provider).await;
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Completed).await;
        let quota = read(&fixture.client).await.provider_quotas.pop().unwrap();
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 0);
        projection
            .apply(0, subscription.next().await.unwrap())
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::RevokeCredential {
                id: quota.credential.id,
                expected_revision: quota.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            changed(&mut subscription, &mut projection, provider.id, false).await,
            None
        );
        assert!(read(&fixture.client).await.provider_quotas.is_empty());
        assert!(credential(&fixture.client, &quota.credential).await.revoked);
        drop(subscription);
        let fixture = renewals::restart(fixture, &server, remote).await;
        assert!(read(&fixture.client).await.provider_quotas.is_empty());
        fixture.close().await;
    }
}
