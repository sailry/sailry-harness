use super::*;

#[test]
fn preserves_script_contracts() {
    for (scenario, check) in [
        (
            "reads_only_public_status_and_returns_drafts",
            (|scenario: &'static str| {
                let output=run(bundle("import {card} from 'sailry/cards'; import {status} from 'sailry/connectors'; export async function run(){return card('connection',{configured:(await status('input'))!==null},{label:'Connect'});}"),"run",json!({}),context(),NodeId([1;32]),CancellationToken::new(),move |request|{
                assert!(matches!(request.command,sailry_protocol::Command::ListConnections), "{scenario}");
                Ok(Output::Connections(Vec::new()))
            }).unwrap();
                assert_eq!(output["kind"], "card", "{scenario}");
                assert!(output.get("receipt").is_none(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "reads_declared_connection_metadata",
            (|scenario: &'static str| {
                let output = run(bundle("import {connectionStatus} from 'sailry/cards'; export async function run() { return connectionStatus('connection'); }"),
        "run", json!({}), context(), NodeId([1;32]), CancellationToken::new(), move |request| {
                    assert!(matches!(request.command, sailry_protocol::Command::ReadPluginConnection { card, .. } if card == "connection"), "{scenario}");
                    Ok(Output::PluginConnection(plugin::connection::Status {
                        package: context().package,
                        card: "connection".into(),
                        server: "input".into(),
                        configuration: plugin::connection::Configuration::Ready,
                        authorization: plugin::connection::Authorization::Missing,
                        credential: plugin::connection::Credential::NotDeclared,
                    }))
                }).unwrap();
                assert_eq!(output["card"], "connection", "{scenario}");
                assert_eq!(output["server"], "input", "{scenario}");
                assert_eq!(output["configuration"], "ready", "{scenario}");
                assert_eq!(output["authorization"], "missing", "{scenario}");
                assert_eq!(output["credential"], "not_declared", "{scenario}");
                assert!(
                    serde_json::to_string(&output)
                        .unwrap()
                        .find("secret")
                        .is_none(),
                    "{scenario}"
                );
            }),
        ),
        (
            "cannot_begin_login_or_expose_native_urls",
            (|scenario: &'static str| {
                let source = "import * as cards from 'sailry/cards'; import {prepareRequest,context} from 'sailry/sdk'; export function run(){ const scope=context(); let code; try { prepareRequest({kind:'start_connector_login',data:{package:scope.package,connector:'service',connection:null,expected_revision:null,method:'mcp_oauth',input:{redirect:'http://127.0.0.1:1/callback'}}}); } catch(error){code=error.code;} return {code,exports:Object.keys(cards).sort()}; }";
                let result = run_tool(
                    bundle(source),
                    "run",
                    json!({}),
                    Environment {
                        context: context(),
                        target: NodeId([1; 32]),
                        stop: CancellationToken::new(),
                        deadline: Instant::now() + CALL_TIMEOUT,
                    },
                    move |_| panic!("{scenario}: headless login must not dispatch"),
                    Arc::new(Mutex::new(execution::State {
                        call_id: Some(sailry_protocol::RequestId::new()),
                        mode: sailry_protocol::WorkMode::Code,
                        read_only: false,
                        turn: None,
                        staged_turn: None,
                    })),
                )
                .unwrap();
                assert_eq!(result["code"], "permission_denied", "{scenario}");
                assert_eq!(
                    result["exports"],
                    json!(["card", "connectionStatus"]),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
