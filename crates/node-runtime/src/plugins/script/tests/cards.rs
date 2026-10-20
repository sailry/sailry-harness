use super::*;

#[test]
fn reads_only_public_status_and_returns_drafts() {
    let output=run(bundle("import {card} from 'sailry/cards'; import {status} from 'sailry/connectors'; export async function run(){return card('connection',{configured:(await status('input'))!==null},{label:'Connect'});}"),"run",json!({}),context(),NodeId([1;32]),CancellationToken::new(),|request|{
        assert!(matches!(request.command,sailry_protocol::Command::ListConnections));
        Ok(Output::Connections(Vec::new()))
    }).unwrap();
    assert_eq!(output["kind"], "card");
    assert!(output.get("receipt").is_none());
}

#[test]
fn reads_declared_connection_metadata() {
    let output = run(bundle("import {connectionStatus} from 'sailry/cards'; export async function run() { return connectionStatus('connection'); }"),
        "run", json!({}), context(), NodeId([1;32]), CancellationToken::new(), |request| {
            assert!(matches!(request.command, sailry_protocol::Command::ReadPluginConnection { card, .. } if card == "connection"));
            Ok(Output::PluginConnection(plugin::connection::Status {
                package: context().package,
                card: "connection".into(),
                server: "input".into(),
                configuration: plugin::connection::Configuration::Ready,
                authorization: plugin::connection::Authorization::Missing,
                credential: plugin::connection::Credential::NotDeclared,
            }))
        }).unwrap();
    assert_eq!(output["card"], "connection");
    assert_eq!(output["server"], "input");
    assert_eq!(output["configuration"], "ready");
    assert_eq!(output["authorization"], "missing");
    assert_eq!(output["credential"], "not_declared");
    assert!(
        serde_json::to_string(&output)
            .unwrap()
            .find("secret")
            .is_none()
    );
}

#[test]
fn cannot_begin_login_or_expose_native_urls() {
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
        |_| panic!("headless login must not dispatch"),
        Arc::new(Mutex::new(execution::State {
            call_id: Some(sailry_protocol::RequestId::new()),
            mode: sailry_protocol::WorkMode::Code,
            read_only: false,
            turn: None,
            staged_turn: None,
        })),
    )
    .unwrap();
    assert_eq!(result["code"], "permission_denied");
    assert_eq!(result["exports"], json!(["card", "connectionStatus"]));
}
