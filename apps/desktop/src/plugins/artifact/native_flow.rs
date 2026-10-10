//! Ordinary Shell, ADK, package host and WebKit acceptance with isolated profiles.
use super::*;
use crate::{agent_fixture::Server, backend::Services, resources::SideResource, shell::Shell};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Authentication, Command, Effort, NodeId, Output, Permission, ProviderId, Session,
    SessionConfig, WorkMode,
    conversation::{Model, ModelApi, Part, Provider},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

const PACKAGE: &str = "brief-native-fixture";
const HTML: &str = r#"<section data-title="Architecture"><h1>Captured resources</h1>
<p class="lead">An isolated Plan document</p>
<pre class="mermaid">flowchart LR
 A["Node"] --> B["Client"]</pre>
<pre class="chart">{"type":"bar","title":"Requests","data":[{"label":"Local","value":3},{"label":"Remote","value":4}]}</pre>
<form data-name="decisions"><fieldset data-name="storage" data-required><legend>Storage</legend>
<label><input type="radio" name="storage" value="node">Execution Node</label>
<label><input type="radio" name="storage" value="project">Project</label></fieldset>
<fieldset data-name="note"><legend>Note</legend><textarea name="note"></textarea></fieldset>
<button type="submit">Send</button></form></section>"#;

#[derive(Default)]
pub(super) struct Surfaces(pub HashMap<(NodeId, ArtifactId), WeakEntity<surface::View>>);
impl Global for Surfaces {}

pub(in crate::plugins) async fn prepare(
    root: &Path,
) -> (Node, Arc<Server>, [(NodeId, Session); 2]) {
    let local = Node::start_isolated(root.join("local")).await.unwrap();
    let remote = Node::start_isolated(root.join("remote")).await.unwrap();
    local
        .link()
        .pair(remote.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let server = Arc::new(Server::scripted_tools(calls(None, None)).await);
    let mut sessions = Vec::new();
    for (name, node) in [("Local HTML brief", &local), ("Remote HTML brief", &remote)] {
        let client = Client::new(node.local());
        let project_root = root.join(name);
        std::fs::create_dir(&project_root).unwrap();
        copy(
            &std::env::var_os("SAILRY_WORKLOAD_PACKAGE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/brief")
                }),
            &project_root.join("brief"),
        );
        let manifest = project_root.join("brief/plugin.json");
        let mut definition: Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        definition["name"] = json!(PACKAGE);
        std::fs::write(manifest, serde_json::to_vec(&definition).unwrap()).unwrap();
        for package in ["context7", "github"] {
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: package.into(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await;
        }
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: name.into(),
                path: project_root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let provider = ProviderId::new();
        execute(
            &client,
            Command::PutProvider {
                expected_revision: 0,
                provider: Provider {
                    id: provider,
                    revision: 0,
                    name: "Isolated document model".into(),
                    api: ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: server.endpoint.clone(),
                    enabled: true,
                    models: vec![Model {
                        id: "fixture".into(),
                        display_name: None,
                        context: 262144,
                        output: 4096,
                        capabilities: None,
                        vision: false,
                        tools: true,
                        reasoning: false,
                        web_search: false,
                        generates: vec![],
                        efforts: vec![],
                        custom_efforts: false,
                        default_effort: Effort::Default,
                    }],
                    default_model: "fixture".into(),
                    credential: None,
                    options: None,
                    oauth: None,
                },
            },
        )
        .await;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    provider,
                    model: "fixture".into(),
                    mode: WorkMode::Plan,
                    permission: Permission::Full,
                    effort: Effort::Default,
                    assistant: None,
                    resource: None,
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::Plugin(package) = execute(
            &client,
            Command::InstallPlugin {
                worktree: session.worktree,
                path: "brief".into(),
                name: PACKAGE.into(),
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("document package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        sessions.push((node.id(), session));
    }
    local.shutdown().await.unwrap();
    (remote, server, sessions.try_into().unwrap())
}

fn copy(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&path, &target);
        } else {
            std::fs::copy(path, target).unwrap();
        }
    }
}

fn calls(id: Option<ArtifactId>, revision: Option<u32>) -> Vec<(String, Value)> {
    vec![(
        crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
        json!({"id":id,"expected_revision":revision,"title":"HTML brief","summary":"Captured resources",
            "html":HTML}),
    )]
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

pub(in crate::plugins) async fn verify(
    output: &Path,
    sessions: [(NodeId, Session); 2],
    server: Arc<Server>,
    shell: Entity<Shell>,
    cx: &mut AsyncWindowContext,
) {
    cx.update(|window, cx| {
        cx.set_global(Surfaces::default());
        rust_i18n::set_locale("en");
        crate::theme::select(Some(gpui_kit::component::ThemeMode::Light), window, cx);
        window.resize(size(px(1440.), px(900.)));
        window.activate_window();
    })
    .unwrap();
    let services = cx
        .read_global::<Services, _>(|services, _, _| services.clone())
        .unwrap();
    let mut records = Vec::new();
    for (route, (node, session)) in ["local", "iroh"].into_iter().zip(sessions) {
        server.replace_tools(calls(None, None));
        until(cx, "known document host", |_, cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&node)
        })
        .await;
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node, cx)
            });
        })
        .unwrap();
        until(cx, "connected document host", |_, cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        })
        .await;
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(session.clone(), window, cx)
            });
        })
        .unwrap();
        let client = cx
            .update(|_, cx| {
                Arc::new(Client::new(
                    shell.read(cx).live.as_ref().unwrap().transport.clone(),
                ))
            })
            .unwrap();
        assert_eq!(client.target(), node);
        let submit = services.runtime.spawn({
            let client = client.clone();
            let session = session.clone();
            async move {
                execute(
                    &client,
                    Command::SubmitTurn {
                        session: session.id,
                        expected_revision: session.revision,
                        message: "Prepare an HTML architecture brief".into(),
                    },
                )
                .await
            }
        });
        assert!(matches!(submit.await.unwrap(), Output::QueuedTurn(_)));
        let first = published(&client, &services, session.id, 1, cx).await;
        finish_plan(&client, &services, &first, cx).await;
        until(cx, "document card in conversation", |_, cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|chat| chat.read(cx).completed_turns() == Some(1))
        })
        .await;
        stage(output, route, "open_card", node, &first);
        until(cx, "document card opened through Shell", |_, cx| {
            matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel))
                if panel.read(cx).document.as_ref().is_some_and(|resource|
                    resource.artifact.id==first.id && resource.binding.client.target()==node
                    && resource.artifact.session==session.id))
        })
        .await;
        let view = mounted(node, first.id, cx).await;
        rendered(&view, cx).await;
        let captured = cx
            .update(|_, cx| {
                let Some(SideResource::Plugin(panel)) = &shell.read(cx).side_resource else {
                    unreachable!()
                };
                panel.read(cx).mounted.as_ref().unwrap().host()
            })
            .unwrap();
        assert_eq!(captured.target(), node);
        assert_eq!(captured.context().session, Some(session.id));
        server.replace_tools(calls(Some(first.id), Some(1)));
        let note = format!("Retained answer {route}");
        evaluate(&view,&format!("document.querySelector('input[value=node]').checked=true;const note=document.querySelector('textarea');note.value={};note.dispatchEvent(new Event('input',{{bubbles:true}}));document.querySelector('form').requestSubmit();true",serde_json::to_string(&note).unwrap()),cx).await;
        let second = published(&client, &services, session.id, 2, cx).await;
        finish_plan(&client, &services, &second, cx).await;
        assert_eq!(second.id, first.id);
        until(cx, "dirty prior revision remains readonly", |_, cx| {
            view.read(cx).fixture_revision() == Some((1, 2))
        })
        .await;
        assert_eq!(
            evaluate(&view, "document.querySelector('textarea').value", cx).await,
            serde_json::to_string(&note).unwrap()
        );
        assert_eq!(
            evaluate(&view, "document.querySelector('textarea').disabled", cx).await,
            "true"
        );
        let history = services
            .runtime
            .spawn({
                let client = client.clone();
                let session = session.clone();
                async move {
                    client
                        .read_conversation(session.id, None, 64)
                        .await
                        .unwrap()
                }
            })
            .await
            .unwrap();
        assert!(
            history
                .page
                .entries
                .iter()
                .filter(|entry| entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .any(
                    |part| matches!(part,Part::Text(text) if text.contains(&note)
                && text.contains(&first.id.to_string()) && text.contains("Execution Node (node)"))
                )
        );
        stage(output, route, "select_version_2", node, &second);
        until(cx, "latest version selected in package picker", |_, cx| {
            view.read(cx).fixture_revision() == Some((2, 2))
        })
        .await;
        rendered(&view, cx).await;
        assert_eq!(
            evaluate(&view, "document.querySelector('textarea').disabled", cx).await,
            "false"
        );
        let entered = format!("Unsaved export values {route}");
        evaluate(&view,&format!("const note=document.querySelector('textarea');note.value={};note.dispatchEvent(new Event('input',{{bubbles:true}}));true",serde_json::to_string(&entered).unwrap()),cx).await;
        pause(cx).await;
        let captured = cx
            .update(|window, cx| view.update(cx, |view, cx| view.capture(2, window, cx)))
            .unwrap()
            .await
            .unwrap();
        assert!(captured.contains(&entered));
        assert!(captured.contains("<svg"));
        assert!(!captured.contains("<script"));
        assert!(!captured.contains("<form"));
        assert!(captured.contains("sailry.dev"));
        std::fs::write(output.join(format!("brief-flow-{route}.html")), captured).unwrap();
        stage(output, route, "select_version_1", node, &first);
        until(
            cx,
            "historical version selected in package picker",
            |_, cx| view.read(cx).fixture_revision() == Some((1, 2)),
        )
        .await;
        rendered(&view, cx).await;
        assert_eq!(
            evaluate(&view, "document.querySelector('textarea').disabled", cx).await,
            "true"
        );
        records.push(json!({"route":route,"node":node,"session":session.id,"artifact":first.id,
            "plan":true,"card_route":true,"package_host":true,"native_surface":true,"form_submit":true,
            "dirty_version_preserved":true,"versions":true,"readonly_history":true,"live_export":true}));
    }
    std::fs::write(
        output.join("brief-flow.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
    std::fs::write(
        output.join("brief-flow-stage.json"),
        "{\"stage\":\"complete\"}\n",
    )
    .unwrap();
    println!(
        "HTML brief flow: real Shell, local/Iroh Plan tools, document cards, native forms, versions and live export verified"
    );
    if let Some(seconds) = std::env::var("SAILRY_WORKLOAD_HOLD")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        cx.background_executor()
            .timer(Duration::from_secs(seconds.min(300)))
            .await;
    }
}

fn stage(output: &Path, route: &str, action: &str, node: NodeId, artifact: &Summary) {
    std::fs::write(output.join("brief-flow-stage.json"),serde_json::to_vec_pretty(&json!({
        "stage":action,"route":route,"node":node,"artifact":artifact.id,"revision":artifact.revision,
        "session":artifact.session,
    })).unwrap()).unwrap();
    println!("HTML brief interaction ready: {route}/{action}");
}

async fn finish_plan(
    client: &Arc<Client>,
    services: &Services,
    artifact: &Summary,
    cx: &mut AsyncWindowContext,
) {
    use sailry_protocol::conversation::{
        Status,
        question::{Answer, Response, State},
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let page = services
            .runtime
            .spawn({
                let client = client.clone();
                let session = artifact.session;
                async move {
                    client
                        .read_conversation(session, None, 64)
                        .await
                        .unwrap()
                        .page
                }
            })
            .await
            .unwrap();
        if page
            .runs
            .iter()
            .any(|run| run.turn == artifact.turn && run.status == Status::Completed)
        {
            return;
        }
        if let Some(question) = page
            .questions
            .iter()
            .find(|question| question.turn == artifact.turn && question.state == State::Pending)
        {
            let call = page
                .entries
                .iter()
                .find(|entry| entry.id == question.entry)
                .and_then(|entry| entry.parts.get(question.index))
                .unwrap();
            assert!(matches!(call, Part::ToolCall { name, .. } if name == "plan_review"));
            services
                .runtime
                .spawn({
                    let client = client.clone();
                    let session = artifact.session;
                    let question = question.id;
                    async move {
                        execute(
                            &client,
                            Command::ResolveQuestion {
                                session,
                                question,
                                response: Response::Answer(Answer::Text(String::new())),
                            },
                        )
                        .await
                    }
                })
                .await
                .unwrap();
        }
        assert!(Instant::now() < deadline, "native Plan completion deadline");
        pause(cx).await;
    }
}

async fn published(
    client: &Arc<Client>,
    services: &Services,
    session: sailry_protocol::SessionId,
    revision: u32,
    cx: &mut AsyncWindowContext,
) -> Summary {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let values = services
            .runtime
            .spawn({
                let client = client.clone();
                async move { client.list_artifacts(session, None, 64).await.unwrap() }
            })
            .await
            .unwrap();
        if let Some(artifact) = values
            .items
            .into_iter()
            .find(|artifact| artifact.revision == revision)
        {
            return artifact;
        }
        assert!(
            Instant::now() < deadline,
            "native artifact publication deadline"
        );
        pause(cx).await;
    }
}

async fn mounted(
    node: NodeId,
    id: ArtifactId,
    cx: &mut AsyncWindowContext,
) -> Entity<surface::View> {
    until(cx, "native package surface mounted", |_, cx| {
        cx.global::<Surfaces>()
            .0
            .get(&(node, id))
            .and_then(WeakEntity::upgrade)
            .is_some_and(|view| view.read(cx).native_fixture().is_some())
    })
    .await;
    cx.update(|_, cx| cx.global::<Surfaces>().0[&(node, id)].upgrade().unwrap())
        .unwrap()
}

async fn rendered(view: &Entity<surface::View>, cx: &mut AsyncWindowContext) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if cx.update(|_,cx|view.read(cx).native_fixture().is_some()).unwrap()
            && evaluate(view,"!!document.querySelector('.document-chart svg') && !!document.querySelector('.document-diagram svg') && !!document.querySelector('textarea')",cx).await=="true" {return}
        assert!(
            Instant::now() < deadline,
            "native package rendering deadline"
        );
        pause(cx).await;
    }
}

async fn evaluate(
    view: &Entity<surface::View>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> String {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    cx.update(|_, cx| {
        view.read(cx)
            .native_fixture()
            .unwrap()
            .page
            .evaluate_script_with_callback(script, move |value| {
                if let Some(sender) = sender.lock().unwrap().take() {
                    let _ = sender.send(value);
                }
            })
            .unwrap()
    })
    .unwrap();
    // Before navigation commits, Wry queues scripts without retaining callbacks.
    // A readiness probe then returns empty and the next poll checks the real DOM.
    receiver.await.unwrap_or_default()
}

async fn until(
    cx: &mut AsyncWindowContext,
    state: &str,
    mut ready: impl FnMut(&mut Window, &mut App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if cx.update(&mut ready).unwrap() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native document flow deadline: {state}"
        );
        pause(cx).await;
    }
}

async fn pause(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    cx.update(|window, cx| window.draw(cx).clear(cx)).unwrap();
}
