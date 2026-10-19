use super::*;

#[tokio::test]
async fn rejects_retargeting_while_approval_is_pending() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let original_path = directory.path().join("original.sqlite3");
        let other_path = directory.path().join("other.sqlite3");
        let original = rusqlite::Connection::open(&original_path).unwrap();
        let other = rusqlite::Connection::open(&other_path).unwrap();
        for db in [&original, &other] {
            db.execute_batch("CREATE TABLE items(value INTEGER)")
                .unwrap();
        }
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let mut profile = save(&client, &original_path, Some(Sharing::Global)).await;
        let call = (
            agent_support::plugin_tool("databases", "database_execute"),
            json!({"connection":profile.id,"sql":"INSERT INTO items VALUES (1)"}),
        );
        let server = Server::tools(vec![call.clone(), call]).await;
        let session = approvals::prepare_packages(&client, &server, &root, &["databases"]).await;
        let turn = approvals::submit(&client, session.id).await;
        let (_, approval) = approvals::pending(&client, session.id).await;
        assert_eq!(count(&original), 0);
        assert_eq!(count(&other), 0);
        profile.connection = database::Connection::Sqlite {
            path: other_path.to_str().unwrap().into(),
        };
        let Output::DatabaseProfile(updated) = client
            .execute(client.prepare(Command::SaveDatabase {
                expected_revision: profile.revision,
                profile,
                password: None,
            }))
            .await
            .unwrap()
        else {
            panic!("database profile expected")
        };
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (page, next) = approvals::pending(&client, session.id).await;
        assert_ne!(next.id, approval.id);
        assert_eq!(count(&original), 0);
        assert_eq!(count(&other), 0);
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(
            |part| matches!(part,Part::ToolResult{result,..} if result["error"]["code"]=="conflict")
        ));
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: next.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(count(&original), 0);
        assert_eq!(count(&other), 1);
        assert_eq!(updated.revision, 2);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn count(db: &rusqlite::Connection) -> i64 {
    db.query_row("SELECT count(*) FROM items", [], |row| row.get(0))
        .unwrap()
}

#[tokio::test]
async fn keeps_script_side_effects_behind_original_confirmation() {
    for remote in [false, true] {
        let model = Server::tools(vec![(
            agent_support::plugin_tool("connection-probe", "execute"),
            json!({}),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        let root = fixture.root.join("package");
        std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
        std::fs::write(
            root.join("dev.sailry.platform/host/main.js"),
            r#"
import {setValue, completeRequest} from 'sailry';
export async function execute() {
  const request=setValue('handler-ran',{ran:true},0);
  await completeRequest(request.id);
  throw new Error('handler executed');
}
"#,
        )
        .unwrap();
        std::fs::write(root.join("plugin.json"),json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"connection-probe","version":"0.1.0",
            "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["databases.control","storage.read","storage.write"],
                "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["execute"]},
                "tools":[{"name":"execute","description":"Exercise connection preparation","handler":{"name":"execute","operation":"databases.execute",
                    "parameters":{"type":"object","properties":{},"additionalProperties":false}}}]}}
        }).to_string()).unwrap();
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "connection-probe".into(),
                expected_revision: 0,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        let read = || {
            fixture
                .client
                .prepare(Command::ReadPluginValue {
                    key: "handler-ran".into(),
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: None,
                    surface: Default::default(),
                    package: package.summary.reference(),
                    worktree: Some(fixture.session.worktree),
                    session: Some(fixture.session.id),
                })
        };
        let Output::PluginValue(before) = fixture.client.execute(read()).await.unwrap() else {
            panic!("value expected")
        };
        assert!(!before.present);
        assert!(before.value.is_null());
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let Output::PluginValue(after) = fixture.client.execute(read()).await.unwrap() else {
            panic!("value expected")
        };
        assert!(!after.present);
        assert!(after.value.is_null());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
