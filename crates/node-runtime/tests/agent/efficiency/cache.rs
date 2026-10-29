//! Explicit billable evidence for the selected Responses model, separate from offline estimates.
use super::*;
use sailry_protocol::conversation::observation::Outcome;
use serde_json::Value;

#[path = "cache/relay.rs"]
mod relay;

const MODEL: &str = "gpt-6-luna";
const OUTPUT_LIMIT: u32 = 8192;
const FILE_MARKER: &str = "FILE_FIRST=Sailry-synthetic-cache-proof";
const SKILL_MARKER: &str = "SKILL_FIRST=Sailry-synthetic-cache-proof";

fn content(root: &std::path::Path) -> (String, String) {
    let file = format!("{FILE_MARKER}\n{}", (1..=160).map(|number|format!("Record {number:03}: synthetic source evidence stays unchanged across all three turns.\n")).collect::<String>());
    std::fs::write(root.join("cache-file.txt"), &file).unwrap();
    std::fs::create_dir_all(root.join("package/skills/analysis")).unwrap();
    std::fs::write(
        root.join("package/plugin.json"),
        json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"cache-fixture","version":"0.1.0"}).to_string(),
    )
    .unwrap();
    let skill = format!("---\nname: analysis\ndescription: Review the synthetic cache acceptance file\n---\n{SKILL_MARKER}\n{}", "Treat file and skill content as captured evidence. Preserve it and answer concisely without editing files.\n".repeat(96));
    std::fs::write(root.join("package/skills/analysis/SKILL.md"), &skill).unwrap();
    (file, skill)
}

async fn prepare(client: &Client, provider: &Provider, session: Session) -> Session {
    approvals::install(client, &["files"]).await;
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree: session.worktree,
            path: "package".into(),
            name: "cache-fixture".into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("synthetic skill package expected")
    };
    assert!(package.issues.is_empty());
    assert!(package.summary.enabled);
    assert!(package.skills.iter().any(|skill| skill.name == "analysis"));
    let Output::Session(session) = client
        .execute(client.prepare(Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: SessionConfig {
                provider: provider.id,
                model: MODEL.into(),
                credential: provider.credential.clone(),
                effort: Effort::High,
                permission: Permission::Full,
                ..session.config
            },
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

#[cfg(unix)]
#[tokio::test]
async fn prepares_without_model_requests() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        content(&fixture.root);
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot
            .providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        provider.api = ModelApi::Responses;
        provider.default_model = MODEL.into();
        let model = &mut provider.models[0];
        model.id = MODEL.into();
        model.output = OUTPUT_LIMIT;
        model.reasoning = true;
        model.efforts = vec![Effort::High];
        model.default_effort = Effort::High;
        model.custom_efforts = true;
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let session = prepare(&fixture.client, &provider, fixture.session).await;
        assert_eq!(session.config.provider, provider.id);
        assert_eq!(session.config.model, MODEL);
        assert_eq!(session.config.effort, Effort::High);
        assert_eq!(session.config.permission, Permission::Full);
        assert!(server.requests.lock().unwrap().is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

fn tokens(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_with_special_tokens(text)
        .len()
}

fn digest(value: &Value) -> String {
    blake3::hash(value.to_string().as_bytes())
        .to_hex()
        .to_string()
}

fn common(left: &str, right: &str) -> Value {
    let mut bytes = left
        .bytes()
        .zip(right.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !left.is_char_boundary(bytes) {
        bytes -= 1;
    }
    json!({"bytes":bytes,"estimated_tokens":tokens(&left[..bytes]),"unchanged":left==right})
}

#[derive(Default)]
struct Prefix {
    system: Option<Value>,
    tools: Option<Value>,
    cache_key: Option<Value>,
}

impl Prefix {
    fn measure(&mut self, request: &relay::Request) -> Value {
        let body = &request.body;
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["max_output_tokens"], OUTPUT_LIMIT);
        let input = body["input"].as_array().expect("complete Responses input");
        let (system, messages): (Vec<_>, Vec<_>) = input
            .iter()
            .partition(|item| matches!(item["role"].as_str(), Some("system" | "developer")));
        let system = json!(system);
        let messages = json!(messages);
        let tools = &body["tools"];
        let cache_key = &body["prompt_cache_key"];
        let context = agent_support::turn_context(body).expect("terminal request context expected");
        let skill = context
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|value| value["skill"] == "cache-fixture:analysis")
            .filter_map(|value| value["content"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        let file: Vec<_> = input
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .filter_map(|item| item["output"].as_str())
            .filter(|text| text.starts_with("cache-file.txt\n"))
            .collect();
        let metric = json!({
            "http_status":request.status,"transport_failed":request.transport_failed,
            "completion":request.completion,"output_limited":request.output_limited,"output_limit":OUTPUT_LIMIT,
            "provider_usage":request.provider_usage,
            "provider_model":request.provider_model,
            "estimated":{"method":"o200k_base over actual serialized request segments; not provider usage; segment counts are not additive",
                "system":tokens(&system.to_string()),"messages":tokens(&messages.to_string()),"tools":tokens(&tools.to_string()),
                "serialized_request":tokens(&request.encoded),"wire_bytes":request.encoded.len(),
                "overlapping_subsets":{"file_results":file.iter().map(|text|tokens(text)).sum::<usize>(),
                    "retained_skill_context":skill.iter().map(|text|tokens(text)).sum::<usize>(),
                    "request_context":tokens(context)}},
            "prefix":{"system_digest":digest(&system),"tools_digest":digest(tools),"cache_key_digest":digest(cache_key),
                "cache_key_present":cache_key.is_string(),
                "system_vs_previous":self.system.as_ref().map(|previous|common(&previous.to_string(),&system.to_string())),
                "tools_unchanged":self.tools.as_ref().map(|previous|previous==tools),
                "cache_key_unchanged":self.cache_key.as_ref().map(|previous|previous==cache_key)},
            "file_marker_occurrences":request.encoded.matches(FILE_MARKER).count(),
            "skill_marker_occurrences":request.encoded.matches(SKILL_MARKER).count(),
            "retained_skill_body_copies":context.matches(SKILL_MARKER).count(),
        });
        self.system = Some(system);
        self.tools = Some(tools.clone());
        self.cache_key = Some(cache_key.clone());
        metric
    }
}

#[tokio::test]
#[ignore = "explicit gpt-6-luna High provider selection; incurs real model usage for local and remote turns"]
async fn reports_selected_model_cache() {
    assert_eq!(
        std::env::var("SAILRY_ACCEPTANCE_MODEL").expect("explicit model required"),
        MODEL
    );
    let provider = std::env::var("SAILRY_ACCEPTANCE_PROVIDER_ID")
        .expect("explicit provider required")
        .parse()
        .unwrap();
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("AGENTS.md"),
            "This isolated synthetic acceptance project must remain unchanged.\n",
        )
        .unwrap();
        let (file, skill) = content(&root);
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let identity = node.id();
        node.shutdown().await.unwrap();
        let mut selected = provider_fixture::seed(MODEL, provider, &profile, identity).await;
        assert_eq!(selected.authentication, Authentication::ApiKey);
        assert_eq!(selected.api, ModelApi::Responses);
        selected
            .models
            .iter_mut()
            .find(|model| model.id == MODEL)
            .unwrap()
            .output = OUTPUT_LIMIT;
        let relay = relay::Relay::start(&selected.endpoint).await;
        selected.endpoint = relay.endpoint.clone();
        let node = Node::start_empty(&profile).await.unwrap();
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
        let Output::Provider(saved) = client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: selected.revision,
                provider: selected.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("isolated provider expected")
        };
        assert_eq!(saved.endpoint, relay.endpoint);
        assert_eq!(saved.credential, selected.credential);
        let (session, _) = configured_session(&client, &saved.endpoint, &root).await;
        let session = prepare(&client, &saved, session).await;
        let prompts = [
            "Load skill cache-fixture:analysis exactly once with load_skill. Read cache-file.txt exactly once with read_file, offset=0 and limit=2000. Reply with only the FILE_FIRST and SKILL_FIRST lines you observed in those results. Preserve all files; use no other tools.",
            "Reuse the unchanged file and loaded skill already in this conversation. Use no tools. Reply with only the exact FILE_FIRST and SKILL_FIRST lines you previously observed.",
            "Check the same retained evidence once more without calling tools. Reply with only the FILE_FIRST and SKILL_FIRST lines from the unchanged file and skill.",
        ];
        let mut prefix = Prefix::default();
        let mut totals = (0u64, 0u64, 0u64);
        let mut replies = 0;
        for (index, prompt) in prompts.into_iter().enumerate() {
            let before = relay.snapshot().len();
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: prompt.into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            assert_eq!(turn.config.model, MODEL);
            assert_eq!(turn.config.effort, Effort::High);
            let outcome = tokio::time::timeout(
                Duration::from_secs(180),
                observe(&client, session.id, turn.id),
            )
            .await;
            if !matches!(&outcome, Ok(Ok(_))) {
                let _ = client
                    .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                    .await;
            }
            let records = events(&node, turn.id).await;
            let requests = observations(&records);
            replies += requests.len();
            let wire = relay.snapshot();
            let wire = &wire[before..];
            let metrics: Vec<_> = wire.iter().map(|request| prefix.measure(request)).collect();
            let mut usage = Vec::new();
            for request in &requests {
                for attempt in &request.attempts {
                    let actual = attempt.usage.as_ref().map(|actual| {
                        assert!(actual.input > 0);
                        assert!(actual.cached_input <= actual.input);
                        totals.0 += actual.input;
                        totals.1 += actual.output;
                        totals.2 += actual.cached_input;
                        json!({"input":actual.input,"output":actual.output,"cached_input":actual.cached_input,"reasoning":actual.reasoning,
                            "hit_percent":100.0*actual.cached_input as f64/actual.input as f64})
                    });
                    usage.push(json!({"usage":actual,
                        "cache_reported":attempt.cache_reported,"reasoning_reported":attempt.reasoning_reported,"outcome":attempt.outcome,
                        "observation_attempt":attempt.index,"transport":attempt.transport,"observation_categories":attempt.categories}));
                }
            }
            println!(
                "{}",
                json!({"suite":"selected-model-cache","model":MODEL,"effort":"high","remote":remote,"turn":index+1,
                "provider_responses":usage,"gdk_calls":requests.len(),"observed_http_attempts":wire.len(),
                "extra_http_attempts":wire.len().saturating_sub(requests.len()),"request_metrics":metrics,
                "tool_names":trace(&records).iter().map(|call|&call["name"]).collect::<Vec<_>>(),"note":"Cache is controlled by the provider. Zero cached tokens is an observation, not failure. Compare post-load system/tools/cache-key prefix stability. GDK retries are unchanged; the relay does not retry or follow redirects."})
            );
            let page = outcome
                .expect("bounded real-model request")
                .expect("real-model turn completed")
                .page;
            assert!(!requests.is_empty());
            assert!(
                requests
                    .iter()
                    .flat_map(|request| &request.attempts)
                    .all(|attempt| attempt.outcome == Outcome::Succeeded && attempt.usage.is_some())
            );
            assert!(wire.iter().all(|request| request.status == Some(200)
                && !request.transport_failed
                && request.completion == Some("completed")
                && !request.output_limited));
            let completed: Vec<_> = wire
                .iter()
                .filter(|request| request.completion == Some("completed"))
                .collect();
            let observed: Vec<_> = requests
                .iter()
                .flat_map(|request| &request.attempts)
                .collect();
            assert_eq!(completed.len(), observed.len());
            for (request, attempt) in completed.into_iter().zip(observed) {
                let actual = attempt.usage.as_ref().unwrap();
                if let Some(input) = request.provider_usage.input_tokens {
                    assert_eq!(actual.input, input);
                }
                if let Some(output) = request.provider_usage.output_tokens {
                    assert_eq!(actual.output, output);
                }
                if let Some(cached) = request.provider_usage.cached_tokens {
                    assert_eq!(actual.cached_input, cached);
                    assert!(attempt.cache_reported);
                }
            }
            let answer: String = page
                .entries
                .iter()
                .filter(|entry| entry.turn == turn.id && entry.author != "user")
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                answer.contains(FILE_MARKER) && answer.contains(SKILL_MARKER),
                "actual retained markers required"
            );
            let calls = trace(&records);
            if index == 0 {
                assert_eq!(
                    calls
                        .iter()
                        .filter(|call| call["name"] == "load_skill")
                        .count(),
                    1
                );
                let results: Vec<_> = blocks(&records)
                    .filter(|block| block["type"] == "tool_response")
                    .collect();
                let original = results
                    .iter()
                    .find(|block| block["name"] == plugin_tool("files", "read_file"))
                    .unwrap();
                assert_eq!(file_text(&original["result"]["value"]), file);
                let original = results
                    .iter()
                    .find(|block| block["name"] == "load_skill")
                    .unwrap();
                assert_eq!(original["result"]["value"]["content"], skill);
                assert_eq!(
                    calls
                        .iter()
                        .filter(|call| call["name"] == plugin_tool("files", "read_file"))
                        .count(),
                    1
                );
            } else {
                assert!(
                    calls.is_empty(),
                    "unchanged evidence must be reused without tools"
                );
            }
        }
        assert_eq!(
            std::fs::read_to_string(root.join("cache-file.txt")).unwrap(),
            file
        );
        assert_eq!(
            std::fs::read_to_string(root.join("package/skills/analysis/SKILL.md")).unwrap(),
            skill
        );
        println!(
            "{}",
            json!({"suite":"selected-model-cache","stage":"complete","model":MODEL,"effort":"high","remote":remote,
            "turns":3,"input":totals.0,"output":totals.1,"cached_input":totals.2,"hit_percent":100.0*totals.2 as f64/totals.0 as f64,
            "output_limit":OUTPUT_LIMIT,
            "gdk_calls":replies,"observed_http_attempts":relay.snapshot().len(),"source_bytes":file.len(),"skill_bytes":skill.len(),
            "source_digest":blake3::hash(file.as_bytes()).to_hex().to_string(),"skill_digest":blake3::hash(skill.as_bytes()).to_hex().to_string()})
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
