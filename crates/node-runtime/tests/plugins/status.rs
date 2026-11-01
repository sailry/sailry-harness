use super::*;
use sailry_client::status::{Scope, View};
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{Context, host::Change};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::watch;

#[cfg(feature = "test-support")]
#[path = "status/execution.rs"]
mod execution;
#[path = "status/fixture.rs"]
mod fixture;
#[path = "status/lifecycle.rs"]
mod lifecycle;
#[path = "status/signals.rs"]
mod signals;

use fixture::{Feed, Fixture};

const SOURCE: &str = r#"
import {context,getValue,getConversationValue,readConversationRuns,setValue,completeRequest} from 'sailry/sdk';
export async function badge(input) {
    const scope=context();
    if (Object.keys(input).length || scope.surface!=='composer' || scope.invocation!=null || scope.turn!=null)
        throw new Error('Unexpected Status callback context');
    const mode=(await getValue('mode')).value;
    if (mode==='undeclared') return [{id:'unknown',visible:true}];
    if (mode==='oversize') return [{id:'badge',label:{label:'x'.repeat(65536)},visible:true}];
    if (mode==='reply') return [{id:'badge',visible:true,reply_to:1}];
    if (mode==='write') {
        const result=await completeRequest(setValue('forbidden',true,'0'));
        if (result.Err) throw Object.assign(new Error(result.Err.message),{code:result.Err.code});
    }
    const counter=(await getValue('counter')).value ?? 0;
    const scoped=scope.session ? (await getConversationValue('scoped')).value ?? 0 : 0;
    const runs=scope.session ? (await readConversationRuns()).runs : [];
    return [{id:'badge',visible:true,enabled:true,label:{label:String(counter)},
        segments:[{label:{label:'Ready',locales:{'zh-CN':'就绪'}},tone:'success'}],
        details:[{label:{label:'Scoped'},value:String(scoped)},{label:{label:'Runs'},value:String(runs.length)},
            {label:{label:'Last run'},value:runs[0]?.status ?? 'none'}]}];
}
export async function invoke(input) {
    if (input.id!=='badge') throw new Error('Unexpected Status entry');
    const counter=await getValue('counter');
    const result=await completeRequest(setValue('counter',(counter.value ?? 0)+1,counter.revision));
    if (result.Err) throw Object.assign(new Error(result.Err.message),{code:result.Err.code});
    if ((await getValue('fail')).value) throw new Error('Status invocation failed');
    return result.Ok.data.value;
}
"#;

#[tokio::test]
async fn updates_scoped_values() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let package = fixture
            .install("status-fixture", SOURCE, &[Change::Package], 0)
            .await;
        let session = fixture.session(None).await;
        let scope = Scope {
            worktree: Some(fixture.worktree),
            session: Some(session.id),
        };
        let context = fixture.context(&package, scope);
        let mut feed = Feed::new(fixture.client.clone(), scope);
        let initial = feed.until(|view| fixture::label(view) == Some("0")).await;
        assert!(initial.connected);
        assert!(initial.failures.is_empty());
        assert_eq!(initial.entries.len(), 2);
        assert!(
            !initial
                .entries
                .iter()
                .find(|entry| entry.declaration.id == "omitted")
                .unwrap()
                .state
                .visible
        );
        assert_eq!(
            initial.entries[0].state.segments[0].label.locales["zh-CN"],
            "就绪"
        );

        fixture.write(&context, "counter", json!(3), 0).await;
        feed.until(|view| fixture::label(view) == Some("3")).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::WritePluginConversationValue {
                        key: "scoped".into(),
                        value: json!(7),
                        expected_revision: 0,
                    })
                    .with_plugin(context),
            )
            .await
            .unwrap();
        let scoped = feed
            .until(|view| fixture::detail(view, "Scoped") == Some("7"))
            .await;
        assert_eq!(fixture::label(&scoped), Some("3"));

        // A session-only subscription resolves its worktree inside the Node.
        let mut session_feed = Feed::new(
            fixture.client.clone(),
            Scope {
                worktree: None,
                session: Some(session.id),
            },
        );
        let resolved = session_feed
            .until(|view| fixture::detail(view, "Scoped") == Some("7"))
            .await;
        assert_eq!(fixture::label(&resolved), Some("3"));
        session_feed.close().await;
        feed.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn invokes_once_and_refreshes_after_failure() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        // Only completion of the declared invocation can invalidate this package.
        let package = fixture.install("status-fixture", SOURCE, &[], 0).await;
        let scope = Scope {
            worktree: Some(fixture.worktree),
            session: None,
        };
        let context = fixture.context(&package, scope);
        let mut feed = Feed::new(fixture.client.clone(), scope);
        let initial = feed.until(|view| fixture::label(view) == Some("0")).await;
        let request = fixture
            .client
            .prepare_status(&scope, &initial.entries[0])
            .unwrap();
        let admitted = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let result = admitted.completion.await.unwrap().unwrap();
        assert_eq!(result, Output::PluginResult(json!(1)));
        feed.until(|view| fixture::label(view) == Some("1")).await;
        assert_eq!(fixture.client.execute(request).await.unwrap(), result);
        assert_eq!(fixture.read(&context, "counter").await.value, json!(1));

        fixture.write(&context, "fail", json!(true), 0).await;
        let current = feed.current();
        let request = fixture
            .client
            .prepare_status(&scope, &current.entries[0])
            .unwrap();
        let failed = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(failed.receipt.durable);
        let error = failed.completion.await.unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        feed.until(|view| fixture::label(view) == Some("2")).await;
        assert_eq!(fixture.client.execute(request).await.unwrap_err(), error);
        let counter = fixture.read(&context, "counter").await;
        assert_eq!((counter.value, counter.revision), (json!(2), 2));
        feed.close().await;
        fixture.close().await;
    }
}
