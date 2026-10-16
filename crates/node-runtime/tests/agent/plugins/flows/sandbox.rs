use super::*;
use sailry_protocol::process::{Access, Outcome};

#[tokio::test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn captures_flow_command_access_before_approval() {
    for remote in [false, true] {
        for (enabled, permission) in [
            (true, Permission::Ask),
            (false, Permission::Ask),
            (false, Permission::Full),
        ] {
            // This outer field belongs to the script, not to its prepared command.
            let arguments = json!({"access":"unrestricted"});
            let server =
                Server::tools(vec![(plugin_tool(PACKAGE, "command"), arguments.clone())]).await;
            let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
            super::super::super::sandbox::permission(&mut fixture, permission).await;
            install(&fixture, r#"
export function command({step,outcome}) {
  if (step === 0) return {call:{operation:'process.run',arguments:{command:'if printf outside > ../flow-outside.txt; then printf unrestricted; else printf confined; fi',access:null,description:null,cwd:'',timeout_ms:1000,background:false,attachments:[]}}};
  return {result:outcome.data,output:0};
}
"#, &[("command", &["process.run"])], None).await;
            let saved = sailry_protocol::sandbox::Settings {
                revision: 0,
                enabled,
            };
            fixture
                .client
                .execute(fixture.client.prepare(Command::SaveSandboxSettings(saved)))
                .await
                .unwrap();
            let turn = submit(&fixture, "Run the declared command flow".into()).await;
            if permission != Permission::Full {
                let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
                assert_eq!(
                    approval.access,
                    Some(if enabled {
                        Access::Workspace
                    } else {
                        Access::Unrestricted
                    })
                );
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SaveSandboxSettings(
                        sailry_protocol::sandbox::Settings {
                            revision: 1,
                            enabled: !enabled,
                        },
                    )))
                    .await
                    .unwrap();
                process::decide(&fixture.client, &approval, Decision::Approve).await;
            }
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert_eq!(page.approvals.len(), 1);
            assert_eq!(
                page.approvals[0].access,
                Some(if enabled {
                    Access::Workspace
                } else {
                    Access::Unrestricted
                })
            );
            let output = results(&page);
            assert_eq!(output.len(), 1, "{page:?}");
            let completion: sailry_protocol::process::Completion =
                serde_json::from_value(output[0].clone()).unwrap();
            assert_eq!(completion.outcome, Outcome::Exited(0));
            assert_eq!(
                completion.stdout.text,
                if enabled { "confined" } else { "unrestricted" }
            );
            assert_eq!(
                fixture.directory.path().join("flow-outside.txt").exists(),
                !enabled
            );
            assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::ToolCall { arguments: original, .. } if original == &arguments)));
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
