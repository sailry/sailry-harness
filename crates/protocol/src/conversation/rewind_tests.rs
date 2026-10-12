use super::*;
use crate::{Event, Output, Session, SessionConfig, WorktreeId};
use serde_json::json;

fn session() -> Session {
    Session {
        id: SessionId::new(),
        archived: false,
        activity: Default::default(),
        project: None,
        worktree: WorktreeId::new(),
        revision: 1,
        config: SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: crate::Effort::Default,
            mode: crate::WorkMode::Code,
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
fn omits_absent_backup() {
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
    assert!(output_wire["data"].get("backup").is_none());
    assert!(event_wire["data"].get("backup").is_none());
    assert_eq!(
        serde_json::from_value::<Output>(output_wire.clone()).unwrap(),
        output
    );
    assert_eq!(
        serde_json::from_value::<Event>(event_wire.clone()).unwrap(),
        event
    );
    output_wire["data"]["backup"] = json!(null);
    event_wire["data"]["backup"] = json!(null);
    assert_eq!(
        serde_json::from_value::<Output>(output_wire).unwrap(),
        output
    );
    assert_eq!(serde_json::from_value::<Event>(event_wire).unwrap(), event);
    assert_eq!(crate::VERSION, 1);
}

#[test]
fn retains_recorded_backup() {
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
    let Output::Rewound(output) = serde_json::from_value::<Output>(output_wire.clone()).unwrap()
    else {
        panic!("rewind output expected")
    };
    assert_eq!(output.backup.as_ref(), Some(&backup));
    assert_eq!(
        serde_json::to_value(Output::Rewound(output)).unwrap(),
        output_wire
    );
    let event = serde_json::from_value::<Event>(event_wire.clone()).unwrap();
    assert!(
        matches!(&event, Event::SessionRewound { backup: Some(value), .. } if **value == backup)
    );
    assert_eq!(serde_json::to_value(event).unwrap(), event_wire);
    assert_eq!(crate::VERSION, 1);
}
