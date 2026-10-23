use super::*;

#[test]
fn queue_outputs_preserve_exact_revisions() {
    use sailry_protocol::conversation::{History, Page, Queue, QueueUpdate};

    let session = sailry_protocol::SessionId::new();
    let queue = Queue {
        revision: 9007199254740993,
        paused: true,
        items: vec![],
    };
    let history = public_output(Output::Conversation(History {
        sequence: 0,
        page: Page {
            session,
            revision: 1,
            entries: vec![],
            runs: vec![],
            queue: queue.clone(),
            approvals: vec![],
            questions: vec![],
            children: vec![],
            background: Vec::new(),
            next_before: None,
        },
        missing: vec![],
    }))
    .unwrap();
    assert_eq!(
        history["data"]["page"]["queue"]["revision"],
        "9007199254740993"
    );
    assert_eq!(history["data"]["page"]["revision"], 1);
    let update = Output::Queue(QueueUpdate {
        session,
        queue,
        runs: vec![],
    });
    assert_eq!(
        public_output(update.clone()).unwrap()["data"]["queue"]["revision"],
        "9007199254740993"
    );
    assert_eq!(
        public_output(Output::PluginTransaction(vec![update])).unwrap()["data"][0]["data"]["queue"]
            ["revision"],
        "9007199254740993"
    );
}

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
        let rejected = complete(&fixture, &module, "completeRequest", json!([stale]));
        assert_eq!(rejected["Err"]["code"], "revision_conflict");
        assert_eq!(
            call(
                &module,
                "isValueConflict",
                json!([{"isError":true,"error":rejected["Err"]}])
            ),
            true
        );
        for outcome in [
            json!({"isError":false,"error":rejected["Err"]}),
            json!({"isError":true,"error":{"code":"revision_conflict"}}),
            json!({"isError":true,"error":{"code":"permission_denied","details":{"kind":"plugin_value_conflict"}}}),
        ] {
            assert_eq!(call(&module, "isValueConflict", json!([outcome])), false);
        }
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

#[test]
fn completes_independent_drafts_together() {
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
        let drafts: Vec<_> = (0..24)
            .map(|index| {
                call(
                    &module,
                    "setValue",
                    json!([format!("item-{index}"), index, "0"]),
                )
            })
            .collect();
        assert_eq!(host.requests.lock().unwrap().len(), drafts.len());
        let pending: Vec<_> = drafts
            .iter()
            .map(|id| module.begin("completeRequest", &args(json!([id]))).unwrap())
            .collect();
        let completed = fixture.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(pending))
                .await
                .expect("concurrent value write deadline")
        });
        for (index, outcome) in completed.into_iter().enumerate() {
            let outcome = decode(&outcome.unwrap()).unwrap();
            assert_eq!(outcome["Ok"]["data"]["revision"], "1");
            assert_eq!(outcome["Ok"]["data"]["value"], index);
        }
        assert_eq!(
            complete(&fixture, &module, "listKeys", json!([]))["keys"]
                .as_array()
                .unwrap()
                .len(),
            drafts.len()
        );
        let oversized = "x".repeat(MAX_COMMAND_BYTES);
        assert!(
            module
                .call("setValue", &args(json!(["large", oversized, "0"])))
                .is_err()
        );
        host.close();
        fixture.close();
    }
}
