use super::super::execute;
use crate::plugins::fixture::Fixture;
use sailry_link::CancellationToken;
use sailry_node_runtime::EnvironmentFixture;
use sailry_protocol::{
    Command, ErrorCode, Output,
    plugin::connector::{Input, Kind, State},
};
use serde_json::json;
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, time::Duration};

#[test]
fn binds_the_current_account_without_an_editor() {
    for remote in [false, true] {
        let fixture = Fixture::with_setup(remote, false, |root| {
            let home = root.join("cli-home");
            fs::create_dir(&home).unwrap();
            let command = home.join("account-fixture");
            fs::write(
                &command,
                concat!(
                    "#!/bin/sh\n",
                    "printf '%s\\n' \"$*\" >> \"$HOME/calls\"\n",
                    "case \"$*\" in\n",
                    "status)\n",
                    "  account=first-account; active=true\n",
                    "  if test -f \"$HOME/switched\"; then account=current-account; fi\n",
                    "  if test -f \"$HOME/signed-out\"; then active=false; fi\n",
                    "  printf '{\"account\":\"%s\",\"active\":%s}\\n' \"$account\" \"$active\";;\n",
                    "'token first-account'|'token current-account')\n",
                    "  : > \"$HOME/entered\"\n",
                    "  while test -f \"$HOME/hold\"; do /bin/sleep 0.01; done\n",
                    "  printf 'synthetic-private-binding-token\\n';;\n",
                    "*) exit 2;;\n",
                    "esac\n",
                ),
            )
            .unwrap();
            fs::set_permissions(&command, fs::Permissions::from_mode(0o700)).unwrap();
            Some(EnvironmentFixture {
                home: home.clone(),
                shell: home.join("missing-shell"),
                environment: BTreeMap::from([("PATH".into(), home.into_os_string())]),
                timeout: Duration::from_secs(1),
            })
        });
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["connectors"] = json!([{
            "id":"account","display":{"label":"Account"},"accounts":"single",
            "origins":["https://service.example.com"],
            "auth":[{"kind":"local_cli","command":"account-fixture",
                "status":{"argv":["status"],"account":"/account",
                    "condition":{"pointer":"/active","values":[true]}},
                "token":{"argv":["token","{account}"],"parse":"line"},
                "timeout_ms":5000,"max_output":1024}],
            "fields":[{"name":"label","title":"Label","default":"Preserved configuration"}],
            "inject":[{"origin":"https://service.example.com","header":"Authorization","prefix":"Bearer "}]
        }]);
        fs::write(&path, manifest.to_string()).unwrap();
        let package = fixture.install(0).summary.reference();
        let client = &fixture.binding.client;
        let start = |connection, expected_revision| {
            client.prepare(Command::StartConnectorLogin {
                package: package.clone(),
                connector: "account".into(),
                connection,
                expected_revision,
                method: Kind::LocalCli,
                input: Input::default(),
            })
        };
        let bind = |request| {
            fixture
                .runtime
                .block_on(execute(client, request, CancellationToken::new()))
        };
        let Output::Connection(original) = bind(start(None, None)).unwrap() else {
            panic!("completed connection expected")
        };
        assert_eq!(original.identity.as_ref().unwrap().id, "first-account");
        let home = fixture.directory.path().join("cli-home");
        fs::write(home.join("switched"), "").unwrap();
        let Output::Connection(current) =
            bind(start(Some(original.id), Some(original.revision))).unwrap()
        else {
            panic!("completed connection expected")
        };
        assert_eq!(current.identity.as_ref().unwrap().id, "current-account");
        assert_eq!(current.fields, original.fields);
        assert_eq!(current.id, original.id);
        assert_eq!(current.state, State::Connected);
        assert!(current.can_disconnect);
        let calls = fs::read_to_string(home.join("calls")).unwrap();
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            [
                "status",
                "token first-account",
                "status",
                "status",
                "token current-account",
                "status",
            ]
        );

        fs::write(home.join("signed-out"), "").unwrap();
        assert_eq!(
            bind(start(Some(current.id), Some(current.revision)))
                .unwrap_err()
                .code,
            ErrorCode::Expired,
        );
        fs::remove_file(home.join("signed-out")).unwrap();
        let Output::Connections(rows) = fixture.execute(Command::ListConnections) else {
            panic!("connections expected")
        };
        assert_eq!(rows[0].identity, current.identity);
        assert_eq!(rows[0].fields, current.fields);
        assert_eq!(rows[0].state, State::Connected);

        fs::remove_file(home.join("entered")).unwrap();
        fs::write(home.join("hold"), "").unwrap();
        let request = start(Some(rows[0].id), Some(rows[0].revision));
        let request_id = request.id;
        fixture.runtime.block_on(async {
            let stop = CancellationToken::new();
            let operation = execute(client, request, stop.clone());
            let cancel = async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !home.join("entered").exists() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                stop.cancel();
            };
            let (result, ()) = tokio::join!(operation, cancel);
            assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        });
        let Output::Connections(restored) = fixture.execute(Command::ListConnections) else {
            panic!("connections expected")
        };
        assert_eq!(restored[0].identity, current.identity);
        assert_eq!(restored[0].fields, current.fields);
        assert_eq!(restored[0].state, State::Connected);
        assert!(fixture.transport.requests.lock().unwrap().iter().any(|request| {
            matches!(request.command, Command::CancelConnectorLogin { attempt } if attempt == request_id)
        }));
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains("synthetic-private-binding-token")
        );
        fixture.close();
    }
}
