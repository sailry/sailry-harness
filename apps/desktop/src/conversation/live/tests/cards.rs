use super::*;
use serde_json::json;

pub(crate) fn install(fixture: &fixture::Fixture) -> sailry_protocol::plugin::Info {
    let root = fixture.directory.path().join("project/card-package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::create_dir_all(root.join("dev.sailry.platform/desktop")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), r#"
import {card} from 'sailry/cards';
export function connection(){return {isError:true,error:{code:'not_configured',message:'Connect'},sailry_content:{version:1,blocks:[card('connection',{payload:'first',expiry:4102444800000},{label:'Connect'})]}};}
"#).unwrap();
    std::fs::write(root.join("dev.sailry.platform/desktop/card.js"), r#"
import {View,div} from 'gpui-kit'; import {state,Action,Qr,nextChange,publishProps} from 'sailry/card';
export default class Card extends View {
  init(_,cx){ this.state=state();this.action=null; cx.spawn(async cx=>{let cursor=this.state.cursor;try{while(true){const update=await nextChange(cursor);cursor=update.cursor;this.state=update;if(update.action&&update.action!==this.action){this.action=update.action;publishProps({...update.props,payload:update.props.payload==='first'?'second':'first'});}cx.notify();}}catch(_){}}); }
  render(){return div().v_flex().child(Qr.new('fixture-qr')).child(Action.new('fixture-refresh',{label:'Refresh',action:{kind:'refresh'}}));}
}
"#).unwrap();
    std::fs::write(root.join("plugin.json"), json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"card-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{
        "api_version":"v1","actions":[],
        "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["connection"]},
        "desktop":{"resources":["dev.sailry.platform/desktop/card.js"]},
        "tools":[{"name":"connection","description":"Read connection","contexts":["workspace"],"presentation":"content","handler":{"name":"connection","read_only":true,"parameters":{"type":"object","properties":{},"additionalProperties":false}}}],
        "cards":[{"key":"connection","entry":"dev.sailry.platform/desktop/card.js",
            "props_schema":{"type":"object","additionalProperties":false,"properties":{"payload":{"type":"string","maxLength":2048},"expiry":{"type":"integer"}},"required":["payload","expiry"]},
            "capabilities":{"refresh":true,"qr":{"payload":"/payload","expires_at":"/expiry"}}}]
    }}}).to_string()).unwrap();
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "card-package".into(),
        name: "card-fixture".into(),
        expected_revision: 0,
        expected_digest: None,
    }) else {
        panic!("card package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

pub(crate) fn submit(fixture: &fixture::Fixture) {
    fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Read the connection".into(),
    });
}

#[gpui::test]
fn restores_cards_in_the_shared_projection(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                support::plugin_tool("card-fixture", "connection"),
                json!({}),
            )],
        );
        install(&fixture);
        submit(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).history.calls.iter().any(|call| {
                call.content.as_ref().is_some_and(|content| {
                    content
                        .blocks
                        .iter()
                        .any(|block| matches!(block, sailry_protocol::tool::Block::Card(_)))
                })
            })
        });
        let canonical = view.read_with(visual, |view, _| view.history.calls.clone());
        assert_eq!(
            view.read_with(visual, |view, _| view.card_instances().len()),
            1
        );
        view.update_in(visual, |view, _, cx| {
            view.suspend_cards(cx);
            cx.notify();
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.history.calls.clone()),
            canonical
        );
        let models = fixture.task_requests();
        view.update_in(visual, |view, _, cx| {
            view.binding.worktree = None;
            view.suspend_cards(cx);
            cx.notify();
        });
        visual.run_until_parked();
        assert!(view.read_with(visual, |view, cx| {
            view.card_instances()
                .iter()
                .all(|card| !card.read(cx).active(cx))
        }));
        view.update_in(visual, |view, _, cx| {
            view.binding.worktree = fixture.binding.worktree;
            view.session.as_mut().unwrap().delegation =
                Some(Box::new(sailry_protocol::Delegation {
                    session: fixture.session.id,
                    turn: sailry_protocol::TurnId::new(),
                    entry: "fixture".into(),
                    index: 0,
                    role: None,
                }));
            view.suspend_cards(cx);
            cx.notify();
        });
        visual.run_until_parked();
        assert!(view.read_with(visual, |view, cx| {
            view.readonly()
                && view
                    .card_instances()
                    .iter()
                    .all(|card| !card.read(cx).active(cx))
        }));
        assert_eq!(
            view.read_with(visual, |view, _| view.history.calls.clone()),
            canonical
        );
        assert_eq!(fixture.task_requests(), models);
        fixture.close();
    }
}
