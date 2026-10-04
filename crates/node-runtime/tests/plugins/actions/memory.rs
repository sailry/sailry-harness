use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[path = "../../memory_support/mod.rs"]
mod support;
use support::{Entry, Kind, MemoryId, Summary};

fn entry(project: Option<ProjectId>, title: &str) -> Entry {
    Entry {
        summary: Summary {
            id: MemoryId::new(),
            project,
            title: title.into(),
            kind: Kind::Feedback,
            revision: 0,
            updated_at_ms: 0,
            archived: false,
        },
        body: format!("Verified {title}"),
    }
}

async fn install(client: &Client, directory: &Path, worktree: WorktreeId) -> Context {
    support::prepare(&directory.join("source/memory"));
    support::install(client, worktree, "memory").await
}

async fn run<T: serde::de::DeserializeOwned>(
    client: &Client,
    context: &Context,
    input: Value,
) -> Result<T, Fault> {
    support::run(client, context, input).await
}

#[tokio::test]
async fn confines_scoped_access_and_private_namespace() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        install(&client, directory.path(), worktree).await;
        let mut context = support::context(&client).await;
        let session = scopes::session(&client, worktree).await;
        context.worktree = Some(worktree);
        context.session = Some(session.id);
        let other_root = directory.path().join("other");
        fs::create_dir(&other_root).unwrap();
        let Output::Project(other) = execute(
            &client,
            Command::RegisterProject {
                name: "Other memory".into(),
                path: other_root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let current = support::put(&client, entry(session.project, "Current constraint"), 0)
            .await
            .unwrap();
        let global = support::put(&client, entry(None, "Global preference"), 0)
            .await
            .unwrap();
        let foreign = support::put(&client, entry(Some(other.id), "Foreign constraint"), 0)
            .await
            .unwrap();
        let all = support::list(&client).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter().map(|entry| entry.id).collect::<BTreeSet<_>>(),
            BTreeSet::from([current.summary.id, global.summary.id, foreign.summary.id])
        );
        let entries: Vec<Summary> = run(
            &client,
            &context,
            json!({"action":"list","project":session.project}),
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.id == current.summary.id));
        assert!(entries.iter().any(|entry| entry.id == global.summary.id));
        assert_eq!(
            run::<Entry>(
                &client,
                &context,
                json!({"action":"read","id":global.summary.id,"project":session.project})
            )
            .await
            .unwrap(),
            global
        );
        let foreign_search:Vec<Entry>=run(&client,&context,json!({"action":"search","project":session.project,"query":foreign.summary.id.to_string()})).await.unwrap();
        assert!(foreign_search.is_empty());
        for input in [
            json!({"action":"read","id":foreign.summary.id,"project":session.project}),
            json!({"action":"remove","id":foreign.summary.id,"expected_revision":1,"project":session.project}),
            json!({"action":"put","entry":foreign,"expected_revision":1,"project":session.project}),
            json!({"action":"put","entry":entry(Some(other.id),"Escaped new record"),"expected_revision":0,"project":session.project}),
            json!({"action":"merge","entry":current,"sources":[{"id":foreign.summary.id,"revision":1}],"project":session.project}),
        ] {
            let request = support::request(&client, &context, input);
            let output = client.execute(request.clone()).await.unwrap();
            let error = support::output::<Value>(output.clone()).unwrap_err();
            assert_eq!(
                error.code,
                ErrorCode::PermissionDenied,
                "{request:?}: {error:?}"
            );
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::Completed(Box::new(Ok(output)))
            );
        }
        assert_eq!(
            support::read(&client, foreign.summary.id).await.unwrap(),
            foreign
        );
        // Another package's identical key is its own value, not access to Memory's catalog.
        let other_context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let key = format!("memory/entry/{}", foreign.summary.id);
        let Output::PluginValue(hidden) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue { key: key.clone() })
                    .with_plugin(other_context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("private value expected")
        };
        assert!(!hidden.present);
        client
            .execute(
                client
                    .prepare(Command::WritePluginValue {
                        key,
                        value: json!({"unrelated":true}),
                        expected_revision: 0,
                    })
                    .with_plugin(other_context),
            )
            .await
            .unwrap();
        assert_eq!(
            support::read(&client, foreign.summary.id).await.unwrap(),
            foreign
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_receipts_and_rechecks_live_authority() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        install(&client, directory.path(), worktree).await;
        let mut context = support::context(&client).await;
        let session = scopes::session(&client, worktree).await;
        context.worktree = Some(worktree);
        context.session = Some(session.id);
        let value = entry(session.project, "Durable constraint");
        let original = support::request(
            &client,
            &context,
            json!({"action":"put","entry":value,"expected_revision":0,"project":session.project}),
        );
        let pending = client.dispatch(original.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&original).await.unwrap()
                {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut changed: Entry = support::output(completed.clone()).unwrap();
        changed.body = "Later user correction".into();
        let saved = support::put(&client, changed.clone(), 1).await.unwrap();
        assert_eq!(client.execute(original.clone()).await.unwrap(), completed);
        assert_eq!(
            support::read(&client, value.summary.id).await.unwrap(),
            saved
        );
        assert_eq!(saved.body, changed.body);
        assert_eq!(saved.summary.revision, 2);
        let mut staged = saved.clone();
        staged.body = "Already prepared replacement".into();
        let draft:Value=run(&client,&context,json!({"action":"prepare","entry":staged,"expected_revision":2,"project":session.project})).await.unwrap();
        let operations = plugin::transaction::sdk::decode(draft["operations"].clone()).unwrap();
        let waiting = client
            .prepare(Command::PluginTransaction { operations })
            .with_plugin(context.clone());
        let mut settings = support::settings(&client).await.unwrap();
        settings.auto_write = false;
        support::save_settings(&client, settings).await.unwrap();
        assert_eq!(
            client.execute(waiting).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(run::<Value>(&client,&context,json!({"action":"remove","id":saved.summary.id,"expected_revision":2,"project":session.project})).await.unwrap_err().code,ErrorCode::PermissionDenied);
        assert_eq!(
            run::<Entry>(
                &client,
                &context,
                json!({"action":"read","id":saved.summary.id,"project":session.project})
            )
            .await
            .unwrap(),
            saved
        );
        let Output::Plugin(package) = execute(
            &client,
            Command::ReadPlugin {
                name: "memory".into(),
            },
        )
        .await
        else {
            panic!("package expected")
        };
        let Output::Plugin(disabled) = execute(
            &client,
            Command::SetPluginEnabled {
                name: "memory".into(),
                expected_revision: package.summary.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("package expected")
        };
        assert_eq!(
            run::<Value>(
                &client,
                &context,
                json!({"action":"list","project":session.project})
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(client.execute(original.clone()).await.unwrap(), completed);
        let mut management = support::context(&client).await;
        management.surface = plugin::desktop::Surface::Settings;
        let Output::PluginValue(settings) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "memory/settings".into(),
                    })
                    .with_plugin(management.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("settings value expected")
        };
        assert!(settings.present);
        assert_eq!(settings.value["settings"]["auto_write"], false);
        assert!(matches!(
            client
                .execute(
                    client
                        .prepare(Command::ReadProjectCatalog)
                        .with_plugin(management.clone())
                )
                .await
                .unwrap(),
            Output::ProjectCatalog(_)
        ));
        // Disabled management does not reopen work outside the settings capabilities.
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadActivityCatalog)
                        .with_plugin(management.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "memory".into(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        assert_eq!(client.execute(original).await.unwrap(), completed);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
