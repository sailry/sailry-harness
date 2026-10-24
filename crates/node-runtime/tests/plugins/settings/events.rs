use super::*;
use plugin::settings::SecretUpdate;
use sailry_protocol::dispatch as jobs;
use std::collections::BTreeMap;

#[tokio::test]
async fn dispatches_committed_revisions_without_ui() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, "Settings");
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let extension = &mut manifest["extensions"]["dev.sailry.platform"];
        extension["scope"] = json!("host");
        extension["actions"] = json!(["dispatch.manage", "storage.read", "storage.write"]);
        extension["host"] = json!({"entry":"dev.sailry.platform/host.js","resources":["dev.sailry.platform/host.js"],"handlers":["configured"]});
        fs::write(path, manifest.to_string()).unwrap();
        fs::write(
            root.join("dev.sailry.platform/host.js"),
            r#"
import {readSettings,getValue,setValue,completeRequest,context} from 'sailry/sdk';
export async function configured(payload) {
  const state=await readSettings();
  if (state.values.endpoint==='failed') throw new Error('fixture callback failed');
  const old=await getValue('observed');
  const result=await completeRequest(setValue('observed',{
    endpoint:state.values.endpoint,reference:context().package,payload
  },old.revision));
  if (result.Err) throw new Error(result.Err.message);
  return {saved:true};
}
"#,
        )
        .unwrap();
        let info = info(execute(&client, install(worktree, 0)).await);
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        let mut context = plugin::Context {
            package: info.summary.reference(),
            worktree: None,
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        };
        dispatch::run(
            &client,
            &context,
            jobs::Command::SaveHandler(jobs::Handler {
                name: "settings".into(),
                revision: 0,
                enabled: true,
                source: jobs::Source {
                    package: context.package.name.clone(),
                    topic: "settings.changed".into(),
                },
                queue: "settings".into(),
                callback: jobs::Callback {
                    scope: jobs::Scope::default(),
                    completion: jobs::Completion::Command,
                    command: Box::new(Command::CallPlugin {
                        handler: "configured".into(),
                        input: json!(null),
                    }),
                    bindings: BTreeMap::from([("/data/input".into(), "/payload".into())]),
                },
            }),
        )
        .await;
        let secret = "fixture-private-settings-not-in-events";
        let request = client.prepare(persistence::save(
            &context.package,
            "https://updated.example.com",
            SecretUpdate::Replace(Secret::new(secret.into())),
        ));
        let saved = persistence::state(client.execute(request.clone()).await.unwrap());
        context.package = saved.package.clone();
        let first = finished(&client, &context, 1).await;
        assert_eq!(
            first[0].status,
            jobs::Status::Completed,
            "{:?}",
            first[0].error
        );
        assert_eq!(first[0].event.source.topic, "settings.changed");
        let mut reference = json!(context.package);
        reference["settings_revision"] = json!(context.package.settings_revision.to_string());
        assert_eq!(
            first[0].event.payload,
            json!({"package":reference,"settings_revision":"1"})
        );
        let Output::PluginValue(observed) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "observed".into(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("observation expected")
        };
        assert_eq!(observed.value["endpoint"], "https://updated.example.com");
        assert_eq!(observed.value["reference"], reference);
        assert_eq!(observed.value["payload"], first[0].event.payload);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::PluginSettings(saved.clone())
        );
        let invalid = client.prepare(Command::SavePluginSettings {
            package: saved.package.clone(),
            values: BTreeMap::from([("endpoint".into(), json!(42))]),
            secrets: BTreeMap::from([("token".into(), SecretUpdate::Keep)]),
        });
        assert_eq!(
            client.execute(invalid).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(persistence::read(&client, &context.package).await, saved);
        let jobs::Output::Jobs(page) = dispatch::run(
            &client,
            &context,
            jobs::Command::ListJobs {
                before: None,
                limit: 100,
            },
        )
        .await
        else {
            panic!("jobs expected")
        };
        assert_eq!(page.jobs.len(), 1);
        let request = client.prepare(persistence::save(
            &context.package,
            "failed",
            SecretUpdate::Keep,
        ));
        let second = persistence::state(client.execute(request.clone()).await.unwrap());
        context.package = second.package.clone();
        let failed = finished(&client, &context, 2).await;
        assert_eq!(failed[0].status, jobs::Status::Failed);
        assert_eq!(persistence::read(&client, &context.package).await, second);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::PluginSettings(second)
        );

        let bodies = node
            .with_storage(|db| {
                Box::pin(async move {
                    let mut rows = db
                        .query("SELECT body FROM dispatch_events ORDER BY sequence", ())
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?;
                    let mut bodies = Vec::new();
                    while let Some(row) = rows
                        .next()
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                    {
                        bodies.push(
                            row.get::<Vec<u8>>(0)
                                .map_err(crate::plugins::database_fixture::fault)?,
                        );
                    }
                    Ok(bodies)
                })
            })
            .await
            .unwrap()
            .into_iter()
            .map(|body| String::from_utf8(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(bodies.len(), 2);
        assert!(bodies.iter().all(|body|!body.contains(secret) && !body.contains("https://updated.example.com")));

        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn finished(client: &Client, context: &plugin::Context, count: usize) -> Vec<jobs::Job> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let jobs::Output::Jobs(page) = dispatch::run(
                client,
                context,
                jobs::Command::ListJobs {
                    before: None,
                    limit: 100,
                },
            )
            .await
            else {
                panic!("jobs expected")
            };
            if page.jobs.len() == count
                && page
                    .jobs
                    .iter()
                    .all(|job| !matches!(job.status, jobs::Status::Queued | jobs::Status::Running))
            {
                return page.jobs;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
