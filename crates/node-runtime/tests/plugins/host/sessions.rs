use super::*;

const SOURCE: &str = r#"
import {listTeams,startSession,listAssociatedSessions,readSession,readConversation,
  setSessionExecution,setSessionConfig,sendMessage,completeRequest,prepareRequest} from 'sailry/sdk';
async function done(id) {
  const result = await completeRequest(id);
  if (result.Err) throw new Error(JSON.stringify(result.Err));
  return result.Ok.data;
}
export async function run(input) {
  const teams = await listTeams();
  const team = teams.find(value => value.id === input.team);
  if (!team || typeof team.revision !== 'string') throw new Error('Exact team catalog required');
  const association = {kind:'schedule',id:'daily'};
  const request = startSession({project:input.project,worktree:input.worktree,config:input.config,
    title:'Callback session',message:{text:'Run',attachments:[]},execution:'team',
    team:{id:team.id,revision:team.revision},association});
  const started = await done(request);
  if ((await done(request)).id !== started.id) throw new Error('Admission replay changed');
  const session = await readSession(started.session);
  const selected = await done(setSessionExecution({execution:'model'},session.revision,session.id));
  const stale = await completeRequest(setSessionExecution({execution:'team'},session.revision,session.id));
  if (stale.Err?.code !== 'revision_conflict') throw new Error('Stale revision accepted');
  const configured = await done(setSessionConfig(selected.config,selected.revision,session.id));
  const sent = await done(sendMessage({text:'Continue',attachments:[]},configured.revision,session.id));
  const history = await readConversation(null,20,session.id);
  const page = await listAssociatedSessions(association,null,1);
  const denied = await completeRequest(prepareRequest({kind:'list_associated_sessions',data:{
    association:{namespace:'other',kind:'schedule',id:'daily'},before:null,limit:1}}));
  if (denied.Err?.code !== 'permission_denied') throw new Error('Foreign association exposed');
  return {started,selected,sent,history:history.page.session,page};
}
"#;

#[tokio::test]
async fn restores_owned_sessions_and_controls_execution() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let config = configuration(&client).await;
        let Output::Team(team) = execute(
            &client,
            Command::PutTeam {
                team: team::Profile {
                    id: TeamId::new(),
                    revision: 0,
                    key: "sdk".into(),
                    name: "SDK".into(),
                    appearance: None,
                    description: String::new(),
                    model: None,
                    instructions: "Complete the task".into(),
                    members: vec![],
                },
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("team expected")
        };
        let actions = serde_json::from_value::<Vec<Action>>(json!([
            "sessions.start",
            "teams.read",
            "conversation.read",
            "conversation.control"
        ]))
        .unwrap();
        let context = installed_with_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            SOURCE,
            0,
            &actions,
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|value| value.id == worktree)
            .unwrap()
            .project;
        let request = call(
            &client,
            &context,
            json!({"project":project,"worktree":worktree,"config":config,"team":team.id}),
        );
        let result = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        let Output::PluginResult(result) = result else {
            panic!("callback result expected")
        };
        assert_eq!(
            result["started"]["roles"]["team"]["revision"],
            team.revision.to_string()
        );
        assert_eq!(result["started"]["roles"]["execution"], "team");
        assert_eq!(result["selected"]["roles"]["execution"], "model");
        assert_eq!(result["sent"]["roles"]["execution"], "model");
        assert_eq!(result["page"]["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(result["page"]["sessions"][0]["id"], result["history"]);
        assert_eq!(
            result["page"]["sessions"][0]["association"]["namespace"],
            "example"
        );
        assert!(result["page"]["sessions"][0]["revision"].is_string());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn configuration(client: &Client) -> SessionConfig {
    let model =
        crate::plugins::completion::configure_empty(client, "http://127.0.0.1:9/v1".into()).await;
    SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: model.split('/').next().unwrap().parse().unwrap(),
        model: "fixture".into(),
        effort: Effort::Default,
        mode: WorkMode::Code,
        options: Default::default(),
        permission: Permission::Ask,
        credential: None,
    }
}

const AUTHORITY: &str = r#"
import {listTeams,listAssociatedSessions,readSession,setSessionExecution,
  completeRequest,prepareRequest} from 'sailry/sdk';
export async function run(input) {
  try {
    if (input.action === 'read') return {Ok:await readSession(input.session)};
    if (input.action === 'lookup') return {Ok:await listAssociatedSessions({kind:'schedule',id:'daily'})};
    if (input.action === 'teams') return {Ok:await listTeams()};
    if (input.action === 'control') return await completeRequest(setSessionExecution({execution:'model'},input.revision,input.session));
    if (input.action === 'file') return await completeRequest(prepareRequest({kind:'read_file',data:{worktree:input.worktree,path:'private.txt'}}));
    throw new Error('Unknown fixture action');
  } catch (error) {
    return {Err:{code:error.code ?? 'unknown',message:error.message}};
  }
}
"#;

async fn associated(
    client: &Client,
    worktree: WorktreeId,
    config: &SessionConfig,
    namespace: &str,
) -> Session {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|value| value.id == worktree)
        .unwrap()
        .project;
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::StartSession(conversation::Start {
            project,
            worktree: Some(worktree),
            config: Some(config.clone()),
            title: "Associated fixture".into(),
            message: "Run".into(),
            execution: conversation::Execution::Model,
            team: None,
            models: Default::default(),
            association: Some(conversation::Association {
                namespace: namespace.into(),
                kind: "schedule".into(),
                id: "daily".into(),
            }),
        }),
    )
    .await
    else {
        panic!("admitted turn expected")
    };
    let Output::Session(session) = execute(
        client,
        Command::ReadSession {
            session: turn.session,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

async fn attempt(
    client: &Client,
    context: &Context,
    input: serde_json::Value,
) -> serde_json::Value {
    let Output::PluginResult(result) = client.execute(call(client, context, input)).await.unwrap()
    else {
        panic!("callback result expected")
    };
    result
}

#[tokio::test]
async fn enforces_association_resource_boundaries() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let config = configuration(&client).await;
        let owned = associated(&client, worktree, &config, "example").await;
        let foreign = associated(&client, worktree, &config, "other").await;
        fs::write(
            directory.path().join("source/private.txt"),
            "Private resource",
        )
        .unwrap();
        let other_root = directory.path().join("other");
        fs::create_dir(&other_root).unwrap();
        let Output::Project(other_project) = execute(
            &client,
            Command::RegisterProject {
                name: "Other project".into(),
                path: other_root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let other_worktree = snapshot
            .worktrees
            .iter()
            .find(|value| value.project == Some(other_project.id))
            .unwrap()
            .id;
        let other = associated(&client, other_worktree, &config, "example").await;
        let actions = serde_json::from_value::<Vec<Action>>(json!([
            "conversation.read",
            "conversation.control",
            "teams.read",
            "files.read"
        ]))
        .unwrap();
        let mut context = installed_with_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            AUTHORITY,
            0,
            &actions,
        )
        .await;
        assert!(context.session.is_none());
        let page = attempt(&client, &context, json!({"action":"lookup"})).await;
        assert_eq!(
            page["Ok"]["sessions"].as_array().unwrap().len(),
            1,
            "{page}"
        );
        assert_eq!(page["Ok"]["sessions"][0]["id"], json!(owned.id));
        for session in [&foreign, &other] {
            for action in ["read", "control"] {
                let denied = attempt(&client, &context, json!({"action":action,"session":session.id,"revision":session.revision.to_string()})).await;
                assert_eq!(denied["Err"]["code"], "permission_denied", "{denied}");
            }
        }
        context.worktree = None;
        let read = attempt(
            &client,
            &context,
            json!({"action":"read","session":owned.id}),
        )
        .await;
        assert_eq!(read["Ok"]["id"], json!(owned.id), "{read}");
        let controlled = attempt(
            &client,
            &context,
            json!({"action":"control","session":owned.id,"revision":owned.revision.to_string()}),
        )
        .await;
        assert_eq!(
            controlled["Ok"]["data"]["id"],
            json!(owned.id),
            "{controlled}"
        );
        let page = attempt(&client, &context, json!({"action":"lookup"})).await;
        assert_eq!(
            page["Ok"]["sessions"].as_array().unwrap().len(),
            2,
            "{page}"
        );
        let denied = attempt(
            &client,
            &context,
            json!({"action":"read","session":foreign.id}),
        )
        .await;
        assert_eq!(denied["Err"]["code"], "permission_denied", "{denied}");
        let denied = attempt(
            &client,
            &context,
            json!({"action":"file","worktree":owned.worktree}),
        )
        .await;
        assert_eq!(denied["Err"]["code"], "permission_denied", "{denied}");
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn requires_each_declared_capability() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        let config = configuration(&client).await;
        let session = associated(&client, worktree, &config, "example").await;
        let capabilities = ["conversation.read", "conversation.control", "teams.read"];
        for (index, (missing, operations)) in [
            ("conversation.read", vec!["read", "lookup"]),
            ("conversation.control", vec!["control"]),
            ("teams.read", vec!["teams"]),
        ]
        .into_iter()
        .enumerate()
        {
            let actions = serde_json::from_value::<Vec<Action>>(json!(
                capabilities
                    .into_iter()
                    .filter(|value| *value != missing)
                    .collect::<Vec<_>>()
            ))
            .unwrap();
            let mut context = installed_with_actions(
                &client,
                &directory.path().join("source/package"),
                worktree,
                AUTHORITY,
                index as u64,
                &actions,
            )
            .await;
            context.worktree = None;
            assert!(context.session.is_none());
            for action in operations {
                let denied = attempt(&client, &context, json!({"action":action,"session":session.id,"revision":session.revision.to_string()})).await;
                assert_eq!(
                    denied["Err"]["code"], "permission_denied",
                    "missing {missing}: {denied}"
                );
            }
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
