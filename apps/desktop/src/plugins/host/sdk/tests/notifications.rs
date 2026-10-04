use super::*;

#[test]
fn public_notifications_use_captured_node_and_receipt() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (package, _server) = fixture.game_plugin_with(
            "city-trader",
            &["player_1_model", "player_2_model"],
            |root| {
                let path = root.join("plugin.json");
                let mut manifest: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                manifest["extensions"]["dev.sailry.platform"]["actions"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("notifications.publish"));
                std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            },
        );
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        let id = call(
            &module,
            "prepareNotification",
            json!([{
                "title":"Reminder", "message":"Check the task", "kind":"info", "session":fixture.session.id,
            }]),
        );
        let first = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(first["Ok"]["kind"], "notification");
        assert_eq!(first["Ok"]["data"]["package"], package.summary.name);
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            first
        );
        host.close();
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.notifications.len(), 1);
        assert_eq!(
            snapshot.notifications[0].content.session,
            Some(fixture.session.id)
        );
        fixture.close();
    }
}
