use super::*;

async fn receipts(node: &Node) -> i64 {
    node.with_storage(move |db| {
        Box::pin(async move {
            db.prepare("SELECT count(*) FROM requests")
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .query_row(())
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .get(0)
                .map_err(crate::plugins::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}

const SOURCE: &str = r#"
import {getValue,setValue,completeRequest} from 'sailry/sdk';
export async function run(input) {
  const entry = await getValue('counter');
  if (input.write) {
    const result = await completeRequest(setValue('counter',(entry.value ?? 0)+1,entry.revision));
    if (result.Err) throw Object.assign(new Error(result.Err.message), {code:result.Err.code});
    return result.Ok.data;
  }
  return entry;
}
"#;

#[tokio::test]
async fn skip_receipts_and_preserve_write_replay() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let context = installed_with_policy(
            &client,
            &directory.path().join("source/package"),
            worktree,
            SOURCE,
            0,
            &[],
            true,
        )
        .await;
        let mut reads = Vec::new();
        for expected in [0, 1] {
            let before = receipts(&node).await;
            for _ in 0..3 {
                let request = client
                    .prepare(Command::ReadPluginCallback {
                        handler: "run".into(),
                        input: json!({}),
                    })
                    .with_plugin(context.clone());
                reads.push(request.id);
                let pending = client.dispatch(request).await.unwrap();
                assert!(!pending.receipt.durable);
                let Output::PluginResult(entry) = pending.completion.await.unwrap().unwrap() else {
                    panic!("entry expected")
                };
                assert_eq!(entry["revision"], expected.to_string());
                assert_eq!(
                    entry["value"],
                    if expected == 0 { json!(null) } else { json!(1) }
                );
            }
            assert_eq!(receipts(&node).await, before);
            if expected == 0 {
                let write = call(&client, &context, json!({"write":true}));
                let first = client.dispatch(write.clone()).await.unwrap();
                assert!(first.receipt.durable);
                let output = first.completion.await.unwrap().unwrap();
                assert_eq!(client.execute(write).await.unwrap(), output);
                assert_eq!(read(&client, &context, "counter").await.value, json!(1));
            }
        }
        let before = receipts(&node).await;
        let denied = client
            .prepare(Command::ReadPluginCallback {
                handler: "run".into(),
                input: json!({"write":true}),
            })
            .with_plugin(context.clone());
        reads.push(denied.id);
        let fault = client.execute(denied).await.unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        let Some(FaultDetails::SystemDiagnostic(report)) = fault.details else {
            panic!("safe callback diagnostic expected");
        };
        let report = *report;
        assert_eq!(read(&client, &context, "counter").await.value, json!(1));
        assert_eq!(receipts(&node).await, before + 1);
        node.with_storage(move |db| {
            Box::pin(async move {
                let body: Vec<u8> = db
                    .prepare("SELECT body FROM requests WHERE id=?1")
                    .await
                    .unwrap()
                    .query_row([report.incident.to_string()])
                    .await
                    .unwrap()
                    .get(0)
                    .unwrap();
                let request: Request = serde_json::from_slice(&body).unwrap();
                assert_eq!(request.id, report.incident);
                let Command::ReportDiagnostic(stored) = request.command else {
                    panic!("only the diagnostic receipt is expected");
                };
                assert_eq!(stored, report);
                Ok(())
            })
        })
        .await
        .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let (db_handle, db) = crate::plugins::database_fixture::open(
            &(directory.path().join("node/storage/node.turso")),
            true,
        )
        .await;
        for request in reads {
            let count: i64 = (async {
                let mut statement = db
                    .prepare("SELECT count(*) FROM requests WHERE id=?1")
                    .await?;
                let row = statement.query_row([request.to_string()]).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            assert_eq!(count, 0);
        }
        crate::plugins::database_fixture::close(db_handle, db).await;
    }
}

#[tokio::test]
async fn undeclared_reads_are_rejected() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            SOURCE,
            0,
        )
        .await;
        let request = client
            .prepare(Command::ReadPluginCallback {
                handler: "run".into(),
                input: json!({"write":true}),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(!read(&client, &context, "counter").await.present);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
