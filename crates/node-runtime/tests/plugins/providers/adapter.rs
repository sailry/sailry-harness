//! Neutral package declarations exercise the shared adapter boundary, not a product plugin.
use super::*;
#[expect(
    dead_code,
    reason = "The shared discovery server also serves other provider contracts"
)]
#[path = "../../discovery_support/mod.rs"]
mod discovery_fixture;

fn declare_adapter(root: &Path, value: u32) {
    package(root, &format!("1.1.{value}"));
    fs::write(
        root.join("dev.sailry.platform/tools.js"),
        "export function echo(input){return input}",
    )
    .unwrap();
    let source = r#"
const version = $VALUE;
export function prepare(input) {
  const messages = [];
  if (input.system) messages.push({role:'system',content:input.system});
  for(const item of input.messages) {
    if(item.role==='tool') messages.push({role:'tool',tool_call_id:item.call_id,content:JSON.stringify(item.parts)});
    else if(item.role==='user') messages.push({role:'user',content:item.parts.map(part=>part.text??'').join('')});
    else messages.push({role:'assistant',content:item.parts.filter(part=>part.kind==='text').map(part=>part.text).join(''),
      tool_calls:item.parts.filter(part=>part.kind==='tool_call').map(part=>({id:part.id,type:'function',function:{name:part.name,arguments:JSON.stringify(part.arguments)}}))});
  }
  return {method:'POST',url:input.endpoint+'/chat/completions',headers:{'content-type':'application/json'},
    body:JSON.stringify({model:input.model,messages,stream:true,translated:version,features:input.features,
      tools:input.tools.map(tool=>({type:'function',function:tool}))}),framing:'sse'};
}
export function decode(input) {
  const deltas=[];
  for(const event of input.events) {
    if(event.data==='[DONE]') continue;
    const chunk=JSON.parse(event.data),choice=chunk.choices?.[0],delta=choice?.delta??{};
    if(delta.content) deltas.push({kind:'text',text:version+':'+delta.content});
    for(const call of delta.tool_calls??[]) deltas.push({kind:'tool_call',index:call.index,id:call.id,name:call.function?.name,arguments_delta:call.function?.arguments});
    if(chunk.usage) deltas.push({kind:'usage',input_tokens:chunk.usage.prompt_tokens,output_tokens:chunk.usage.completion_tokens});
    if(choice?.finish_reason) deltas.push({kind:'finish',reason:choice.finish_reason});
  }
  return {state:input.state,deltas};
}
export function classify(){return {class:'unavailable',message:'Synthetic failure'}}
"#.replace("$VALUE",&value.to_string());
    fs::write(root.join("dev.sailry.platform/adapter.js"), source).unwrap();
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","actions":["models.provide"],
        "host":{"entry":"dev.sailry.platform/tools.js","resources":["dev.sailry.platform/tools.js"],"handlers":["echo"]},
        "tools":[{"name":"echo","description":"Echo neutral input","handler":{"name":"echo","read_only":true,"parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}}}],
        "model_adapters":[{"id":"example","script":{"entry":"dev.sailry.platform/adapter.js"},"origins":[],"framing":"sse","schema_dialect":"open_ai","capabilities":{"tools":true,"reasoning":true}}],
        "model_providers":[{"id":"example","name":"Example adapter","routes":[{"protocol":"plugin","endpoint":"http://localhost:11434/v1"}],"adapter":"example"}]}});
    fs::write(path, manifest.to_string()).unwrap();
}

#[tokio::test]
async fn runs_captured_translations_without_manual_approval() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package");
        declare_adapter(&source, 1);
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let server = model_fixture::Server::tools(vec![(
            model_fixture::plugin_tool("example", "echo"),
            json!({"query":"original"}),
        )])
        .await;
        let provider:Provider=serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Example adapter","api":"plugin","authentication":"api_key","endpoint":server.endpoint,"enabled":true,"models":[],"default_model":"","credential":null,"preset":{"source":{"kind":"package","data":{"name":"example"}},"id":"example"}})).unwrap();
        let Output::Provider(mut provider) = execute(
            &client,
            Command::PutProvider {
                provider,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|item| item.id == worktree)
            .unwrap()
            .project;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project,
                worktree: Some(worktree),
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    credential: None,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Full,
                    options: Default::default(),
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let queue = || Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Translate using captured resources".into(),
        };
        assert!(provider.consent.is_none());
        let Output::QueuedTurn(first) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        finish(&client, session.id, first.id).await;
        let original = captured(node.profile(), first.id);
        assert!(
            original
                .binding
                .as_ref()
                .unwrap()
                .adapter
                .as_ref()
                .unwrap()
                .capabilities
                .tools
        );
        assert_eq!(
            original.binding.as_ref().unwrap().package,
            Some(installed.summary.reference())
        );
        {
            let requests = server.requests.lock().unwrap();
            assert!(
                requests.len() >= 2,
                "tool continuation must invoke the translator again"
            );
            assert!(requests.iter().all(|request| request["translated"] == 1));
            assert!(
                requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["role"] == "tool"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.contains("original")))
            );
        }
        declare_adapter(&source, 2);
        let updated = info(execute(&client, install(worktree, installed.summary.revision)).await);
        // Updating code does not add a per-version approval step.
        let Output::QueuedTurn(second) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        finish(&client, session.id, second.id).await;
        assert_eq!(
            captured(node.profile(), second.id).binding.unwrap().package,
            Some(updated.summary.reference())
        );
        assert_eq!(
            server.requests.lock().unwrap().last().unwrap()["translated"],
            2
        );
        assert_eq!(captured(node.profile(), first.id), original);
        provider.consent = None;
        execute(
            &client,
            Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            },
        )
        .await;
        let count = server.requests.lock().unwrap().len();
        let Output::QueuedTurn(third) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        finish(&client, session.id, third.id).await;
        assert!(server.requests.lock().unwrap().len() > count);
        assert!(captured(node.profile(), third.id).consent.is_none());
        assert_eq!(captured(node.profile(), first.id), original);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn corrects_arguments_without_executing_invalid_calls() {
    use sailry_protocol::conversation::Part;

    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package");
        declare_adapter(&source, 1);
        fs::write(
            source.join("dev.sailry.platform/tools.js"),
            r#"
import {getValue,setValue,completeRequest,forgetRequest} from 'sailry/sdk';
export async function echo(input) {
  const before = await getValue('executions');
  const id = setValue('executions',(before.value??0)+1,before.revision);
  await completeRequest(id);
  forgetRequest(id);
  return input;
}
"#,
        )
        .unwrap();
        let path = source.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] =
            json!(["models.provide", "storage.read", "storage.write"]);
        manifest["extensions"]["dev.sailry.platform"]["tools"][0]["handler"]["read_only"] =
            false.into();
        fs::write(path, manifest.to_string()).unwrap();
        let path = source.join("dev.sailry.platform/adapter.js");
        let translation = fs::read_to_string(&path).unwrap().replace(
            "export function prepare(input) {",
            "export function prepare(input) { if(input.model==='unavailable') throw new Error('Synthetic prepare failure');",
        );
        assert_eq!(
            translation
                .matches("arguments_delta:call.function?.arguments")
                .count(),
            1
        );
        fs::write(path,translation.replace("arguments_delta:call.function?.arguments",
            "arguments_delta:call.function?.arguments==='{\"query\":\"broken\"}'?'{':call.function?.arguments")).unwrap();
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let name = model_fixture::plugin_tool("example", "echo");
        let server = model_fixture::Server::scripted_tools(vec![
            (name.clone(), json!({"query":"broken"})),
            (name.clone(), json!({"query":"corrected"})),
        ])
        .await;
        let provider: Provider = serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Example adapter","api":"plugin","authentication":"api_key","endpoint":server.endpoint,"enabled":true,"models":[],"default_model":"","credential":null,"preset":{"source":{"kind":"package","data":{"name":"example"}},"id":"example"}})).unwrap();
        let Output::Provider(provider) = execute(
            &client,
            Command::PutProvider {
                provider,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|item| item.id == worktree)
            .unwrap()
            .project;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project,
                worktree: Some(worktree),
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    credential: None,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Full,
                    options: Default::default(),
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let message = "Keep this original task while correcting the tool call";
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: message.into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        finish(&client, session.id, turn.id).await;
        let page = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page;
        let original = page
            .entries
            .iter()
            .find(|entry| entry.turn == turn.id && entry.author == "user")
            .unwrap()
            .clone();
        assert!(original.parts.contains(&Part::Text(message.into())));
        let calls = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn.id)
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolCall { id, arguments, .. } => Some((id, arguments)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0.as_deref(), Some("call-fixture-0"));
        assert!(adk_core::invalid_tool_arguments_reason(calls[0].1).is_some());
        assert_eq!(calls[0].1[adk_core::INVALID_TOOL_ARGUMENTS_KEY]["raw"], "{");
        assert_eq!(calls[1].0.as_deref(), Some("call-fixture-1"));
        assert_eq!(calls[1].1, &json!({"query":"corrected"}));
        assert!(calls.iter().all(|(_, arguments)| arguments.is_object()));
        let results = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn.id)
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { id, result, .. } => Some((id, result)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.as_deref(), Some("call-fixture-0"));
        assert!(
            results[0].1["error"]
                .as_str()
                .unwrap()
                .starts_with(&format!("tool '{name}' was not run:"))
        );
        assert_eq!(results[1].0.as_deref(), Some("call-fixture-1"));
        assert!(
            results[1].1.to_string().contains("corrected"),
            "{:?}",
            results[1].1
        );
        let context = plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: installed.summary.reference(),
            worktree: Some(worktree),
            session: Some(session.id),
        };
        let Output::PluginValue(executions) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "executions".into(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("plugin value expected")
        };
        assert_eq!(executions.value, json!(1));
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            let tools = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["role"] == "tool")
                .collect::<Vec<_>>();
            assert_eq!(tools.len(), 1);
            assert!(
                tools[0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("was not run")
            );
            assert!(requests[2].to_string().contains("corrected"));
        }
        let Output::Session(session) = execute(
            &client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: SessionConfig {
                    model: "unavailable".into(),
                    ..session.config.clone()
                },
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let failed_message = "Retain this request after the adapter fails";
        let Output::QueuedTurn(failed) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: failed_message.into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let failed_page = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let page = client
                    .read_conversation(session.id, None, 100)
                    .await
                    .unwrap()
                    .page;
                if let Some(run) = page.runs.iter().find(|run| run.turn == failed.id)
                    && matches!(
                        run.status,
                        sailry_protocol::conversation::Status::Completed
                            | sailry_protocol::conversation::Status::Failed
                    )
                {
                    assert_eq!(run.status, sailry_protocol::conversation::Status::Failed);
                    assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::Internal);
                    break page;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(failed_page.entries.contains(&original));
        assert!(
            failed_page
                .entries
                .iter()
                .filter(|entry| entry.turn == failed.id)
                .flat_map(|entry| &entry.parts)
                .all(|part| !matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. }))
        );
        let failed_input = failed_page
            .entries
            .iter()
            .find(|entry| entry.turn == failed.id && entry.author == "user")
            .unwrap()
            .clone();
        assert!(
            failed_input
                .parts
                .contains(&Part::Text(failed_message.into()))
        );
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        server.replace_tools(vec![]);
        let Output::Session(session) = execute(
            &client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: SessionConfig {
                    model: "continued".into(),
                    ..session.config.clone()
                },
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(next) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Continue the original task".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        assert_eq!(next.config.model, "continued");
        finish(&client, session.id, next.id).await;
        let page = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page;
        assert!(page.entries.contains(&original));
        assert!(page.entries.contains(&failed_input));
        assert!(
            failed_page
                .entries
                .iter()
                .all(|entry| page.entries.contains(entry))
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 4);
            let request = requests.last().unwrap();
            assert_eq!(request["model"], "continued");
            assert!(request.to_string().contains(message));
            assert!(request.to_string().contains(failed_message));
            assert!(request.to_string().contains("corrected"));
        }
        let Output::PluginValue(retained) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "executions".into(),
                    })
                    .with_plugin(context),
            )
            .await
            .unwrap()
        else {
            panic!("plugin value expected")
        };
        assert_eq!(retained, executions);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn enable_discovery(root: &Path) {
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["model_adapters"][0]["discovery"] = true.into();
    fs::write(path, manifest.to_string()).unwrap();
    let path = root.join("dev.sailry.platform/adapter.js");
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str(r#"
export function prepare_discovery(input) {
  if(Object.keys(input).some(key=>key!=='endpoint')) throw new Error('Unexpected discovery input');
  return {method:'GET',url:input.endpoint+'/metadata',headers:{},body:null,
    credential:{header:'authorization',template:'Bearer {key}'},framing:'json'};
}
export function decode_discovery(input) {
  if(input.status!==200 || JSON.stringify(input).includes('private-adapter-discovery-key')) throw new Error('Unexpected discovery response');
  return {endpoint:input.endpoint,models:JSON.parse(input.body).data};
}
"#);
    fs::write(path, source).unwrap();
}

fn stored_state(profile: &Path) -> (i64, Vec<[u8; 32]>) {
    let db = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let requests = db
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    let mut query = db
        .prepare("SELECT body FROM providers ORDER BY id")
        .unwrap();
    let providers = query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|body| *blake3::hash(&body.unwrap()).as_bytes())
        .collect();
    (requests, providers)
}

async fn discover(
    client: &Client,
    source: sailry_protocol::conversation::discovery::Source,
) -> sailry_protocol::conversation::discovery::Catalog {
    let request = client.prepare(Command::DiscoverModels(Box::new(source)));
    assert!(!request.command.durable());
    let admission = client.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::DiscoveredModels(catalog) = admission.completion.await.unwrap().unwrap() else {
        panic!("discovered models expected")
    };
    catalog
}

#[tokio::test]
async fn discovers_without_message_access() {
    use sailry_protocol::conversation::discovery;
    const KEY: &str = "private-adapter-discovery-key";
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package");
        declare_adapter(&source, 1);
        enable_discovery(&source);
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let server=discovery_fixture::Server::start_with_request(ModelApi::Plugin,|request| {
            assert_eq!(request.method,"GET");
            assert_eq!(request.path,"/v1/metadata");
            assert!(request.body.is_empty());
            assert_eq!(request.headers.get("authorization").map(String::as_str),Some("Bearer private-adapter-discovery-key"));
            discovery_fixture::Reply::Json(json!({"data":[{"id":"adapter-model","display_name":"Adapter model","context":4096,"output":128,"capabilities":{"tools":true}}]}))
        }).await;
        let reference = PresetRef {
            source: Source::Package {
                name: "example".into(),
            },
            id: "example".into(),
            route: None,
        };
        let before = stored_state(node.profile());
        let draft = discover(
            &client,
            discovery::Source::Draft(discovery::Draft {
                provider: ProviderId::new(),
                api: ModelApi::Plugin,
                preset: Some(reference.clone()),
                endpoint: server.endpoint.clone(),
                credential: None,
                secret: Some(Secret::new(KEY.into())),
            }),
        )
        .await;
        assert_eq!(stored_state(node.profile()), before);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(draft.endpoint, server.endpoint);
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "adapter-model");
        assert_eq!(draft.models[0].context, Some(4096));
        assert_eq!(draft.models[0].output, Some(128));
        assert_eq!(
            draft.models[0].capabilities.as_ref().unwrap().tools,
            Some(true)
        );
        let provider:Provider=serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Discovery adapter","api":"plugin","authentication":"api_key","endpoint":server.endpoint,"enabled":true,"models":[],"default_model":"","credential":null,"preset":reference})).unwrap();
        let Output::Provider(provider) = execute(
            &client,
            Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new(KEY.into())),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        assert!(provider.consent.is_none());
        assert!(provider.credential.is_some());
        let before = stored_state(node.profile());
        let saved = discover(
            &client,
            discovery::Source::Saved {
                provider: provider.id,
                expected_revision: provider.revision,
                oauth: None,
            },
        )
        .await;
        assert_eq!(saved, draft);
        assert_eq!(stored_state(node.profile()), before);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn captures_declared_feature_choices() {
    use sailry_protocol::conversation::discovery;
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package");
        declare_adapter(&source, 1);
        enable_discovery(&source);
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let server = discovery_fixture::Server::start_with_request(ModelApi::Plugin, |request| {
            if request.path == "/v1/metadata" {
                return discovery_fixture::Reply::Json(json!({"data":[{"id":"fixture","context":4096,"output":128,
                    "capabilities":{"tools":true,"features":[{"id":"speed","name":"Speed","options":[{"id":"fast","name":"Fast"}]}]}}]}));
            }
            assert_eq!(request.path,"/v1/chat/completions");
            let body:serde_json::Value = serde_json::from_str(&request.body).unwrap();
            assert!(matches!(body["features"]["speed"].as_str(),Some("fast")|None));
            let chunk = json!({"id":"fixture","model":"fixture","choices":[{"index":0,"delta":{"content":"Completed"},"finish_reason":"stop"}]});
            discovery_fixture::Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {chunk}\n\ndata: [DONE]\n\n"))
        }).await;
        let reference = PresetRef {
            source: Source::Package {
                name: "example".into(),
            },
            id: "example".into(),
            route: None,
        };
        let catalog = discover(
            &client,
            discovery::Source::Draft(discovery::Draft {
                provider: ProviderId::new(),
                api: ModelApi::Plugin,
                preset: Some(reference.clone()),
                endpoint: server.endpoint.clone(),
                credential: None,
                secret: Some(Secret::new("private-adapter-discovery-key".into())),
            }),
        )
        .await;
        assert_eq!(
            catalog.models[0].capabilities.as_ref().unwrap().features[0].options[0].id,
            "fast"
        );
        let mut provider:Provider = serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Feature adapter","api":"plugin","authentication":"api_key","endpoint":server.endpoint,"enabled":true,"models":[],"default_model":"fixture","credential":null,"preset":reference})).unwrap();
        provider.models = catalog
            .models
            .iter()
            .map(|model| {
                model.configuration(
                    None,
                    ModelApi::Plugin,
                    sailry_protocol::Authentication::ApiKey,
                )
            })
            .collect();
        let Output::Provider(provider) = execute(
            &client,
            Command::PutProvider {
                provider,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == worktree)
            .unwrap()
            .project;
        let Output::Session(mut session) = execute(
            &client,
            Command::CreateSession {
                project,
                worktree: Some(worktree),
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    features: std::collections::BTreeMap::from([("speed".into(), "fast".into())]),
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    options: Default::default(),
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        for enabled in [true, false] {
            if !enabled {
                let previous = session.revision;
                let mut config = session.config.clone();
                config.features.clear();
                let Output::Session(updated) = execute(
                    &client,
                    Command::SetSessionConfig {
                        session: session.id,
                        expected_revision: previous,
                        config: config.clone(),
                    },
                )
                .await
                else {
                    panic!("session expected")
                };
                session = updated;
                assert_eq!(
                    client
                        .execute(client.prepare(Command::SetSessionConfig {
                            session: session.id,
                            expected_revision: previous,
                            config,
                        }))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::RevisionConflict
                );
            }
            let Output::QueuedTurn(turn) = execute(
                &client,
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Translate declared choices".into(),
                },
            )
            .await
            else {
                panic!("queued turn expected")
            };
            assert_eq!(
                turn.config.features.get("speed").map(String::as_str),
                enabled.then_some("fast")
            );
            finish(&client, session.id, turn.id).await;
            assert_eq!(
                captured(node.profile(), turn.id).models[0]
                    .capabilities
                    .as_ref()
                    .unwrap()
                    .features,
                catalog.models[0].capabilities.as_ref().unwrap().features
            );
        }
        {
            let requests = server.requests.lock().unwrap();
            let bodies: Vec<serde_json::Value> = requests
                .iter()
                .filter(|request| request.path == "/v1/chat/completions")
                .map(|request| serde_json::from_str(&request.body).unwrap())
                .collect();
            assert_eq!(bodies.len(), 2);
            assert_eq!(bodies[0]["features"], json!({"speed":"fast"}));
            assert_eq!(bodies[1]["features"], json!({}));
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
