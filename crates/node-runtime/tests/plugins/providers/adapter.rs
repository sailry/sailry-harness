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
    body:JSON.stringify({model:input.model,messages,stream:true,translated:version,
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
async fn runs_captured_translations_with_scoped_approval() {
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
        assert_eq!(
            client
                .execute(client.prepare(queue()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(server.requests.lock().unwrap().is_empty());
        provider.consent = Some(Box::new(sailry_protocol::conversation::consent::Approval {
            package: installed.summary.reference(),
            origin: None,
            scopes: sailry_protocol::conversation::consent::Scopes {
                messages: true,
                credentials: false,
            },
        }));
        let Output::Provider(saved) = execute(
            &client,
            Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        provider = saved;
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
        let count = server.requests.lock().unwrap().len();
        assert_eq!(
            client
                .execute(client.prepare(queue()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(server.requests.lock().unwrap().len(), count);
        provider.consent.as_mut().unwrap().package = updated.summary.reference();
        provider.consent.as_mut().unwrap().origin = None;
        let Output::Provider(saved) = execute(
            &client,
            Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        provider = saved;
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
        assert_eq!(
            client
                .execute(client.prepare(queue()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(server.requests.lock().unwrap().len(), count);
        assert_eq!(captured(node.profile(), first.id), original);
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
