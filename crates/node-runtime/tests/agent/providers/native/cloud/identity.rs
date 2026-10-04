use super::*;
#[allow(dead_code)]
#[path = "../../../../discovery_support/mod.rs"]
mod exchange;

#[test]
fn isolates_host_credentials() {
    for kind in ["bedrock", "vertex"] {
        let directory = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "providers::native::cloud::identity::uses_execution_host_identity",
                "--ignored",
                "--nocapture",
            ])
            .env("SAILRY_IDENTITY_FIXTURE", kind)
            .env(
                "GOOGLE_APPLICATION_CREDENTIALS",
                directory.path().join("adc.json"),
            )
            .env("AWS_ACCESS_KEY_ID", "ISOLATED_ACCESS_KEY")
            .env("AWS_SECRET_ACCESS_KEY", "isolated-signing-secret")
            .env(
                "AWS_SHARED_CREDENTIALS_FILE",
                directory.path().join("aws-credentials"),
            )
            .env("AWS_CONFIG_FILE", directory.path().join("aws-config"))
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .env_remove("AWS_PROFILE")
            .env_remove("AWS_SESSION_TOKEN")
            .env_remove("AWS_BEARER_TOKEN_BEDROCK")
            .env_remove("AWS_WEB_IDENTITY_TOKEN_FILE")
            .env_remove("AWS_ROLE_ARN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{kind}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
#[ignore = "launched only by the isolated host-identity subprocess harness"]
async fn uses_execution_host_identity() {
    let kind = std::env::var("SAILRY_IDENTITY_FIXTURE")
        .expect("host-identity fixture must be launched by its isolated subprocess harness");
    let api = if kind == "bedrock" {
        ModelApi::Bedrock
    } else {
        ModelApi::Vertex
    };
    let exchange = exchange::Server::start_with_request(ModelApi::Anthropic, |request| {
        match request.path.as_str() {
            "/subject" => exchange::Reply::Json(serde_json::json!({"subject":"isolated-subject-token"})),
            "/token" => exchange::Reply::Json(serde_json::json!({"access_token":"isolated-host-access-token","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"Bearer","expires_in":3600})),
            _ => panic!("unexpected credential exchange route"),
        }
    }).await;
    if api == ModelApi::Vertex {
        let credential = serde_json::json!({
            "type":"external_account", "audience":"fixture-audience",
            "subject_token_type":"urn:ietf:params:oauth:token-type:jwt",
            "token_url":format!("{}/token", exchange.endpoint),
            "credential_source":{"url":format!("{}/subject", exchange.endpoint), "format":{"type":"json", "subject_token_field_name":"subject"}}
        });
        std::fs::write(
            std::env::var("GOOGLE_APPLICATION_CREDENTIALS").unwrap(),
            credential.to_string(),
        )
        .unwrap();
    }
    for remote in [false, true] {
        let server = Server::start(api, Reply::Text).await;
        let mut fixture = Fixture::new(remote, api, &server.endpoint).await;
        let mut provider = fixture.provider.clone();
        provider.authentication = Authentication::Host;
        provider.credential = None;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut config = fixture.session.config.clone();
        config.credential = None;
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.submit("Host identity fixture").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{api:?}: {:?}",
            page.runs
        );
        assert!(text(&page).contains(server::ANSWER));
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        let auth = &requests[0].headers["authorization"];
        if api == ModelApi::Bedrock {
            assert!(auth.starts_with("AWS4-HMAC-SHA256 "));
            assert!(auth.contains("Credential=ISOLATED_ACCESS_KEY/"));
            assert!(auth.contains("/us-east-1/bedrock/aws4_request"));
        } else {
            assert_eq!(auth, "Bearer isolated-host-access-token");
            assert!(!requests[0].headers.contains_key("x-goog-api-key"));
        }
        let mut invalid = provider.clone();
        invalid.options = None;
        let request = fixture.client.prepare(Command::PutProvider {
            expected_revision: invalid.revision,
            provider: invalid,
        });
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        let request = fixture.client.prepare(Command::SaveProvider {
            expected_revision: provider.revision,
            provider: provider.clone(),
            secret: Some(Secret::new("must-not-store".into())),
        });
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        let request = fixture.client.prepare(Command::BeginProviderLogin {
            provider: provider.id,
            expected_revision: provider.revision,
        });
        assert!(fixture.client.execute(request).await.is_err());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
