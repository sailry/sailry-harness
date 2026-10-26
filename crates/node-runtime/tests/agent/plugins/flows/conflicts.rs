use super::*;

#[tokio::test]
async fn replans_only_rolled_back_private_transactions() {
    for remote in [false, true] {
        let calls = ["retry", "explain", "escape", "bounded"];
        let server = Server::tools(
            calls
                .iter()
                .map(|name| (plugin_tool(PACKAGE, name), json!({})))
                .collect(),
        )
        .await;
        let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
        let package=install(&fixture,r#"
import {getValue,isValueConflict,callId} from 'sailry/sdk';
const write=(key,revision=0)=>({kind:'write',data:{key,value:'committed',expected_revision:revision}});
const call=(operations,state=null)=>({call:{operation:'storage.transaction',arguments:{operations}},state});
export async function retry({step,outcome,state}) {
  if (step===0) return call([write('partial'),write('head',99)],{id:callId()});
  if (isValueConflict(outcome)) {
    const absent=!(await getValue('partial')).present, head=await getValue('head');
    return call([write('partial'),write('head',head.revision)],{...state,absent});
  }
  return {result:{absent:state.absent,stable:state.id===callId(),values:outcome.data},output:step-1};
}
export function explain({step}) {
  if (step===0) return call([write('explained',99)]);
  return {result:{isError:false,error:null,saved:true,context:{status:'changed',saved:false,latest:'current'}},output:0};
}
export function escape({step}) {
  if (step===0) return call([write('escape',99)]);
  return {call:{operation:'storage.set',arguments:{key:'replayed',value:'forbidden',expected_revision:0}}};
}
export function bounded({step,outcome}) {
  if (step===8 || (step>0 && !isValueConflict(outcome))) return {result:{context:{status:'pending',saved:false,attempts:step}}};
  return call([write('bounded',99)]);
}
"#,&[("retry",&["storage.transaction"]),("explain",&["storage.transaction"]),
    ("escape",&["storage.transaction","storage.set"]),("bounded",&["storage.transaction"])],None).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let turn = submit(&fixture, "Exercise atomic conflict handling".into()).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let output = results(&fixture.client, &page).await;
        assert_eq!(output.len(), 4, "{page:?}");
        assert_eq!(output[0]["absent"], true);
        assert_eq!(output[0]["stable"], true);
        let view =
            super::super::super::progress::observe(&fixture.client, fixture.session.id).await;
        assert!(view.calls[0].failure.is_none());
        for call in &view.calls[1..] {
            assert_eq!(
                call.failure.as_ref().and_then(|failure| failure.code),
                Some(ErrorCode::RevisionConflict)
            );
            assert!(call.details(&page));
        }
        for key in ["partial", "head"] {
            assert_eq!(value(&fixture, &package, key).await.revision, 1);
        }
        for result in &output[1..] {
            assert_eq!(result["isError"], true);
            assert_eq!(result["error"]["code"], "revision_conflict");
            assert_eq!(
                result["error"]["details"],
                json!({"kind":"plugin_value_conflict"})
            );
            assert!(result.get("saved").is_none());
        }
        assert_eq!(
            output[1]["context"],
            json!({"status":"changed","saved":false,"latest":"current"})
        );
        assert!(output[2].get("context").is_none());
        assert_eq!(
            output[3]["context"],
            json!({"status":"pending","saved":false,"attempts":8})
        );
        for key in ["explained", "escape", "replayed", "bounded"] {
            assert!(!value(&fixture, &package, key).await.present);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn does_not_resume_revoked_package_authority() {
    for remote in [false, true] {
        let server = Server::tools(vec![(plugin_tool(PACKAGE, "revoke"), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        let package=install(&fixture,r#"
const call=key=>({call:{operation:'storage.transaction',arguments:{operations:[{kind:'write',data:{key,value:true,expected_revision:0}}]}}});
export function revoke({step}) {return call(step===0 ? 'original' : 'replayed');}
"#,&[("revoke",&["storage.transaction"])],None).await;
        let (entered, release) = fixture.node.hold_plugin_operation_fixture();
        let turn = submit(&fixture, "Use the captured package".into()).await;
        let _prepared = tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        let Output::Plugin(disabled) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: PACKAGE.into(),
                expected_revision: package.summary.revision,
                enabled: false,
            }))
            .await
            .unwrap()
        else {
            panic!("disabled package expected")
        };
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let output = results(&fixture.client, &page).await;
        assert_eq!(output.len(), 1, "{page:?}");
        assert_eq!(output[0]["isError"], true);
        assert_ne!(
            output[0]["error"]["details"],
            json!({"kind":"plugin_value_conflict"})
        );
        let Output::Plugin(enabled) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: PACKAGE.into(),
                expected_revision: disabled.summary.revision,
                enabled: true,
            }))
            .await
            .unwrap()
        else {
            panic!("enabled package expected")
        };
        for key in ["original", "replayed"] {
            assert!(!value(&fixture, &enabled, key).await.present);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
