use super::*;

#[test]
fn reads_only_public_status_and_returns_drafts() {
    let output = run(bundle("import {card,status} from 'sailry/cards'; export async function run() { return card('connection',await status('input'),{label:'Connect'}); }"),
        "run", json!({}), context(), NodeId([1;32]), CancellationToken::new(), |request| {
            assert!(matches!(request.command, sailry_protocol::Command::ReadMcpAuthorization { server, .. } if server == "input"));
            Ok(Output::McpAuthorization(plugin::authorization::Status { package:context().package, server:"input".into(), configured:false }))
        }).unwrap();
    assert_eq!(
        output,
        json!({"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":"Connect"}})
    );
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
    let source = "import * as cards from 'sailry/cards'; import {prepareRequest,context} from 'sailry/sdk'; export function run(){ const scope=context(); let code; try { prepareRequest({kind:'begin_mcp_login',data:{package:scope.package,server:'input',redirect:'http://127.0.0.1:1/callback',client_id:null}}); } catch(error){code=error.code;} return {code,exports:Object.keys(cards).sort()}; }";
    let result = run(
        bundle(source),
        "run",
        json!({}),
        context(),
        NodeId([1; 32]),
        CancellationToken::new(),
        |_| panic!("headless login must not dispatch"),
    )
    .unwrap();
    assert_eq!(result["code"], "permission_denied");
    assert_eq!(
        result["exports"],
        json!(["card", "connectionStatus", "status"])
    );
}
