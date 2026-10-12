use super::*;

#[test]
fn requires_native_actions() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0).summary.reference();
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                package: package.clone(),
                surface: sailry_protocol::plugin::desktop::Surface::Settings,
                worktree: None,
                session: None,
                turn: None,
                invocation: None,
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        for command in [
            Command::BeginPluginRegistration { package },
            Command::CancelPluginLogin {
                attempt: RequestId::new(),
            },
        ] {
            assert!(
                host.prepare_public(command.clone())
                    .unwrap_err()
                    .to_string()
                    .contains("native user action")
            );
            let mut value = serde_json::to_value(command).unwrap();
            if let Some(package) = value["data"].get_mut("package") {
                package["settings_revision"] = json!("0");
            }
            assert!(
                host.sdk()
                    .call("prepareRequest", &args(json!([value.clone()])))
                    .is_err()
            );
            assert!(
                host.module()
                    .call("prepare", &args(json!([value.to_string()])))
                    .is_err()
            );
        }
        assert!(host.requests.lock().unwrap().is_empty());
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(
                    request.command,
                    Command::BeginPluginRegistration { .. } | Command::CancelPluginLogin { .. }
                ))
        );
        host.close();
        fixture.close();
    }
}
