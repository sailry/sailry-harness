use super::*;

#[test]
fn sends_exact_provider_scope_without_durable_receipts() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0).summary.reference();
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.clone(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        module.validate().unwrap();
        let provider = sailry_protocol::ProviderId::new();
        let error = fixture
            .runtime
            .block_on(
                module
                    .begin(
                        "requestProviderHttp",
                        &args(json!([
                            provider, "9007199254740993", "account", {"date":"2026-10-19"}
                        ])),
                    )
                    .unwrap(),
            )
            .unwrap_err();
        let fault: sailry_protocol::Fault = serde_json::from_str(&error.to_string()).unwrap();
        assert_eq!(fault.code, sailry_protocol::ErrorCode::PermissionDenied);
        let requests = fixture.transport.requests.lock().unwrap();
        let request = requests
            .iter()
            .find(|request| matches!(request.command, Command::RequestProviderHttp { .. }))
            .expect("provider HTTP request expected");
        let Command::RequestProviderHttp {
            provider: actual,
            expected_revision,
            request: name,
            input,
        } = &request.command
        else {
            unreachable!()
        };
        assert_eq!(*actual, provider);
        assert_eq!(*expected_revision, 9007199254740993);
        assert_eq!(name, "account");
        assert_eq!(input.get("date").map(String::as_str), Some("2026-10-19"));
        assert_eq!(request.plugin.as_ref().unwrap().package, package);
        assert!(host.requests.lock().unwrap().is_empty());
        assert!(
            module
                .begin(
                    "requestProviderHttp",
                    &args(json!([provider, "01", "account", {}]))
                )
                .is_err()
        );
        assert!(
            module
                .begin(
                    "requestProviderHttp",
                    &args(json!([
                        provider,"1","account",{"date":42}
                    ]))
                )
                .is_err()
        );
    }
}
