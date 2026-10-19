use super::*;
use crate::plugins::web::server;

const SOURCE: &str = r#"
import {fetchWeb,getValue,setValue,completeRequest} from 'sailry/sdk';
export async function run(input) {
    if (input.url) return await fetchWeb({url:input.url});
    if (input.write) {
        const entry=await getValue('counter');
        const result=await completeRequest(setValue('counter',(entry.value ?? 0)+1,entry.revision));
        if (result.Err) throw Object.assign(new Error(result.Err.message),{code:result.Err.code});
        return result.Ok;
    }
    return await getValue('counter');
}
"#;

#[tokio::test]
async fn cancels_reads_and_releases_vm_slots() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let hosts = ["first.sailry.test", "second.sailry.test"];
        for host in hosts {
            node.set_web_fixture(host, http.address);
        }
        node.set_plugin_deadline_fixture(Duration::from_millis(500));
        let context = installed_with_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            SOURCE,
            0,
            &[Action::ReadWeb],
        )
        .await;
        let mut pending = Vec::new();
        for host in hosts {
            pending.push(
                client
                    .dispatch(call(
                        &client,
                        &context,
                        json!({
                            "url":format!("http://{host}:{}/hold",http.address.port())
                        }),
                    ))
                    .await
                    .unwrap(),
            );
        }
        // The server must observe both requests; merely polling Pending would
        // not establish that the VMs reached native SDK I/O.
        tokio::time::timeout(Duration::from_secs(5), async {
            while http.requests.lock().unwrap().len() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        for admission in pending {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), admission.completion)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::Cancelled
            );
        }
        tokio::time::timeout(Duration::from_secs(5), http.disconnected.cancelled())
            .await
            .unwrap();
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            client.execute(call(&client, &context, json!({"write":true}))),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(output, Output::PluginResult(_)));
        assert_eq!(read(&client, &context, "counter").await.value, json!(1));
        assert_eq!(http.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_admitted_writes_after_deadline() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_plugin_deadline_fixture(Duration::from_millis(500));
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            SOURCE,
            0,
        )
        .await;
        let (entered, release) = node.hold_plugin_completion_fixture();
        let callback = call(&client, &context, json!({"write":true}));
        let pending = client.dispatch(callback.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        let original = tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(original.command, Command::WritePluginValue { .. }));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        release.cancel();
        let digest = blake3::hash(&serde_json::to_vec(&original).unwrap())
            .to_hex()
            .to_string();
        let outcome = client
            .execute(client.prepare(Command::InspectRequest {
                id: original.id,
                digest,
            }))
            .await
            .unwrap();
        let Output::RequestOutcome {
            id,
            outcome: RequestOutcome::Completed(result),
        } = outcome
        else {
            panic!("original write must retain its real durable result");
        };
        assert_eq!(id, original.id);
        let actual = result.unwrap();
        assert_eq!(client.execute(original).await.unwrap(), actual);
        assert_eq!(read(&client, &context, "counter").await.value, json!(1));
        assert_eq!(read(&client, &context, "counter").await.revision, 1);
        assert_eq!(
            client.execute(callback).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        let Output::PluginResult(entry) = client
            .execute(call(&client, &context, json!({})))
            .await
            .unwrap()
        else {
            panic!("released VM must accept a new callback");
        };
        assert_eq!(entry["value"], json!(1));
        assert_eq!(entry["revision"], json!("1"));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
