use super::*;
use crate::{Event, Output, Session, SessionConfig, WorktreeId};
use serde_json::json;

fn session() -> Session {
    Session {
        association: None,
        id: SessionId::new(),
        creation_order: 1,
        archived: false,
        activity: Default::default(),
        project: None,
        worktree: WorktreeId::new(),
        revision: 1,
        config: SessionConfig {
            features: Default::default(),
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: crate::Effort::Default,
            mode: crate::WorkMode::Code,
            options: Default::default(),
            permission: crate::Permission::Ask,
            credential: None,
        },
        roles: Default::default(),
        profile: None,
        fork: None,
        delegation: None,
    }
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "omits_absent_backup",
            (|scenario: &'static str| {
                let source = session();
                let output = Output::Rewound(Box::new(Rewind {
                    session: source.id,
                    revision: 2,
                    through: Some(TurnId::new()),
                    backup: None,
                }));
                let event = Event::SessionRewound {
                    session: Box::new(source),
                    backup: None,
                };
                let mut output_wire = serde_json::to_value(&output).unwrap();
                let mut event_wire = serde_json::to_value(&event).unwrap();
                assert!(output_wire["data"].get("backup").is_none(), "{scenario}");
                assert!(event_wire["data"].get("backup").is_none(), "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Output>(output_wire.clone()).unwrap(),
                    output,
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Event>(event_wire.clone()).unwrap(),
                    event,
                    "{scenario}"
                );
                output_wire["data"]["backup"] = json!(null);
                event_wire["data"]["backup"] = json!(null);
                assert_eq!(
                    serde_json::from_value::<Output>(output_wire).unwrap(),
                    output,
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Event>(event_wire).unwrap(),
                    event,
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "retains_recorded_backup",
            (|scenario: &'static str| {
                let source = session();
                let backup = Session {
                    id: SessionId::new(),
                    ..source.clone()
                };
                let through = TurnId::new();
                let output_wire = json!({"kind":"rewound","data":{
                    "session":source.id,"revision":2,"through":through,"backup":backup
                }});
                let event_wire = json!({"kind":"session_rewound","data":{
                    "session":source,"backup":backup
                }});
                let Output::Rewound(output) =
                    serde_json::from_value::<Output>(output_wire.clone()).unwrap()
                else {
                    panic!("{scenario}: rewind output expected")
                };
                assert_eq!(output.backup.as_ref(), Some(&backup), "{scenario}");
                assert_eq!(
                    serde_json::to_value(Output::Rewound(output)).unwrap(),
                    output_wire,
                    "{scenario}"
                );
                let event = serde_json::from_value::<Event>(event_wire.clone()).unwrap();
                assert!(
                    matches!(&event, Event::SessionRewound { backup: Some(value), .. } if **value == backup),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::to_value(event).unwrap(),
                    event_wire,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
