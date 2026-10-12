use super::*;
use sailry_protocol::{
    Command,
    installation::{Resource, State, Task},
    models::Kind,
};

#[test]
fn preserves_model_scope_and_exact_revisions() {
    let scope = context();
    let expected = scope.clone();
    let captured = Arc::new(Mutex::new(Vec::<Request>::new()));
    let records = captured.clone();
    let result = run(
        bundle(
            r#"
        import * as models from 'sailry/models';
        import * as installations from 'sailry/installations';
        import {completeRequest,forgetRequest} from 'sailry/sdk';
        export async function run() {
            const id = models.prepare({target:'node',kind:'embedding',model:'granite-multi'});
            await completeRequest(id);
            forgetRequest(id);
            const tasks = await installations.read([id]);
            await installations.dismiss(id);
            return tasks;
        }
    "#,
        ),
        "run",
        json!(null),
        scope,
        NodeId([1; 32]),
        CancellationToken::new(),
        move |request| {
            assert_eq!(request.plugin.as_ref(), Some(&expected));
            records.lock().unwrap().push(request.clone());
            match request.command {
                Command::InstallKnowledgeModel { model } => Ok(Output::KnowledgeModel { model }),
                Command::ReadInstallations { requests } => Ok(Output::Installations(vec![Task {
                    request: requests[0],
                    revision: 9_007_199_254_740_993,
                    resource: Resource::Model {
                        kind: Kind::Embedding,
                        id: "granite-multi".into(),
                    },
                    state: State::Failed {
                        error: Fault::invalid("native fixture failure"),
                    },
                }])),
                Command::DismissInstallation { request } => {
                    Ok(Output::InstallationDismissed { request })
                }
                _ => panic!("unexpected SDK command"),
            }
        },
    )
    .unwrap();
    assert_eq!(result[0]["revision"], "9007199254740993");
    assert_eq!(result[0]["target"], "node");
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let Command::ReadInstallations { requests: ids } = &requests[1].command else {
        panic!("observation expected")
    };
    assert_eq!(ids, &[requests[0].id]);
}

#[test]
fn rejects_foreign_requests_and_controller_targets() {
    for source in [
        "import * as installations from 'sailry/installations'; export async function run() { try { return await installations.read(['00000000-0000-4000-8000-000000000001']); } catch(error) { return error.code; } }",
        "import * as installations from 'sailry/installations'; export async function run() { try { return await installations.dismiss('00000000-0000-4000-8000-000000000001'); } catch(error) { return error.code; } }",
    ] {
        assert_eq!(
            execute(source, CancellationToken::new(), Duration::from_secs(2)).unwrap(),
            json!("permission_denied")
        );
    }
    for source in [
        "import * as models from 'sailry/models'; export function run() { try { return models.prepare({target:'controller',kind:'speech',id:null}); } catch(error) { return error.code; } }",
        "import * as installations from 'sailry/installations'; export function run() { try { return installations.watch([]); } catch(error) { return error.code; } }",
        "import * as installations from 'sailry/installations'; export async function run() { try { return await installations.next(); } catch(error) { return error.code; } }",
    ] {
        assert_eq!(
            execute(source, CancellationToken::new(), Duration::from_secs(2)).unwrap(),
            json!("unavailable")
        );
    }
}

#[test]
fn does_not_expose_package_installation() {
    let source = "import {prepareRequest} from 'sailry/sdk'; export function run() { try { return prepareRequest({kind:'install_bundled_plugin',data:{name:'example',expected_revision:0}}); } catch(error) { return error.code; } }";
    assert_eq!(
        execute(source, CancellationToken::new(), Duration::from_secs(2)).unwrap(),
        json!("permission_denied")
    );
}
