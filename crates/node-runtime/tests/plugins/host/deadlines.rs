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
async fn times_out_reads_and_releases_slots() {
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
            let fault = tokio::time::timeout(Duration::from_secs(5), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(fault.code, ErrorCode::Expired);
            let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) = fault.details
            else {
                panic!("typed timeout expected");
            };
            assert_eq!(
                report.reason,
                sailry_protocol::notification::DiagnosticReason::Timeout
            );
            assert_eq!(
                report.phase,
                sailry_protocol::notification::DiagnosticPhase::Sdk
            );
            assert_eq!(report.operation.as_deref(), Some("fetch_web"));
            assert!(report.request.is_some());
            assert!(report.budget_ms.unwrap() <= 500);
            assert!(report.elapsed_ms.unwrap() >= 400);
            assert_eq!(report.plugin.unwrap().name, "example");
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        let reports: Vec<_> = snapshot
            .notifications
            .iter()
            .filter(|notice| {
                matches!(
                    notice.content.details,
                    Some(sailry_protocol::notification::Details::System(_))
                )
            })
            .collect();
        assert_eq!(reports.len(), 2);
        let serialized = serde_json::to_string(&reports).unwrap();
        assert!(!serialized.contains("sailry.test"));
        assert!(!serialized.contains("/hold"));
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
    for (remote, cancel) in [(false, false), (true, false), (false, true), (true, true)] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        if !cancel {
            node.set_plugin_deadline_fixture(Duration::from_millis(500));
        }
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
        if cancel {
            client
                .execute(
                    client
                        .prepare(Command::CancelPluginCall {
                            request: callback.id,
                        })
                        .with_plugin(context.clone()),
                )
                .await
                .unwrap();
        }
        let fault = tokio::time::timeout(Duration::from_secs(5), pending.completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::OutcomeUnknown);
        let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) = &fault.details else {
            panic!("typed uncertain completion expected");
        };
        assert_eq!(
            report.reason,
            if cancel {
                sailry_protocol::notification::DiagnosticReason::Cancelled
            } else {
                sailry_protocol::notification::DiagnosticReason::Timeout
            }
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        let reports: Vec<_> = snapshot
            .notifications
            .iter()
            .filter(|notice| {
                matches!(
                    notice.content.details,
                    Some(sailry_protocol::notification::Details::System(_))
                )
            })
            .collect();
        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].content.kind,
            sailry_protocol::notification::Kind::Warning
        );
        assert_eq!(report.request, Some(original.id));
        assert_eq!(report.operation.as_deref(), Some("write_plugin_value"));
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

#[tokio::test]
async fn explicit_stop_is_not_a_timeout() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_web_fixture("stopped.sailry.test", http.address);
        let context = installed_with_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            SOURCE,
            0,
            &[Action::ReadWeb],
        )
        .await;
        let request = call(
            &client,
            &context,
            json!({"url":format!("http://stopped.sailry.test:{}/hold",http.address.port())}),
        );
        let pending = client.dispatch(request.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while http.requests.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        client
            .execute(
                client
                    .prepare(Command::CancelPluginCall {
                        request: request.id,
                    })
                    .with_plugin(context),
            )
            .await
            .unwrap();
        let fault = tokio::time::timeout(Duration::from_secs(5), pending.completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::Cancelled);
        let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) = fault.details else {
            panic!("typed stop expected");
        };
        assert_eq!(
            report.reason,
            sailry_protocol::notification::DiagnosticReason::Cancelled
        );
        assert_eq!(report.operation.as_deref(), Some("fetch_web"));
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert!(!snapshot.notifications.iter().any(|notice| matches!(
            notice.content.details,
            Some(sailry_protocol::notification::Details::System(_))
        )));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn failing_package_does_not_poison_another() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let root = directory.path().join("source/package");
        let failed = installed(&client, &root, worktree, "export function run() { globalThis.contaminated=true; throw new Error('PRIVATE_SCRIPT_BODY'); }", 0).await;
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["name"] = json!("healthy");
        fs::write(path, manifest.to_string()).unwrap();
        fs::write(root.join("dev.sailry.platform/main.js"), "export function run() { return { healthy: typeof globalThis.contaminated === 'undefined' }; }").unwrap();
        let package = info(
            execute(
                &client,
                Command::InstallPlugin {
                    worktree,
                    path: "package".into(),
                    name: "healthy".into(),
                    expected_revision: 0,
                    expected_digest: None,
                },
            )
            .await,
        );
        let healthy = Context {
            package: package.summary.reference(),
            ..failed.clone()
        };
        let (failure, success) = tokio::join!(
            client.execute(call(&client, &failed, json!(null))),
            client.execute(call(&client, &healthy, json!(null)))
        );
        assert_eq!(failure.unwrap_err().code, ErrorCode::InvalidRequest);
        assert_eq!(
            success.unwrap(),
            Output::PluginResult(json!({"healthy":true}))
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        let reports: Vec<_> = snapshot
            .notifications
            .iter()
            .filter(|notice| {
                matches!(
                    notice.content.details,
                    Some(sailry_protocol::notification::Details::System(_))
                )
            })
            .collect();
        assert_eq!(reports.len(), 1);
        assert!(
            !serde_json::to_string(&reports)
                .unwrap()
                .contains("PRIVATE_SCRIPT_BODY")
        );
        assert_eq!(
            client
                .execute(call(&client, &healthy, json!(null)))
                .await
                .unwrap(),
            Output::PluginResult(json!({"healthy":true}))
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
