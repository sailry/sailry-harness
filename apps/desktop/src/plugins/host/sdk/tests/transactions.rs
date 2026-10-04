use super::*;

#[test]
fn commits_scoped_values_and_notices_atomically() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::Plugin(package) = fixture.execute(Command::ReadPlugin {
            name: "reminders".into(),
        }) else {
            panic!("plugin expected")
        };
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: None,
                session: None,
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        let operations = json!([
            {"kind":"write","data":{"key":"item","value":true,"expected_revision":"0"}},
            {"kind":"notify","data":{"title":"Saved","message":"Updated","kind":"info","session":null}},
        ]);
        let id = call(&module, "prepareTransaction", json!([operations]));
        let result = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(result["Ok"]["kind"], "plugin_transaction");
        assert_eq!(result["Ok"]["data"][0]["data"]["revision"], "1");
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            result
        );
        let stale = call(&module, "prepareTransaction", json!([operations]));
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([stale]))["Err"]["code"],
            "revision_conflict"
        );
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.notifications.len(), 1);
        assert!(
            module
                .call(
                    "prepareTransaction",
                    &args(json!([[{"kind":"write_file","data":{}}]]))
                )
                .is_err()
        );
        host.close();
        assert!(
            module
                .call("prepareTransaction", &args(json!([operations])))
                .is_err()
        );
        fixture.close();
    }
}
