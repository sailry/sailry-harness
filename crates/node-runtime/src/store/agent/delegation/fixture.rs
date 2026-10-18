use super::*;
use crate::store::agent::tests::command;
pub(super) use crate::store::agent::tests::fixture::Fixture;

pub(super) fn setup() -> Fixture {
    let fixture = Fixture::new();
    for name in ["delegation", "files"] {
        crate::store::agent::tests::fixture::package(&fixture.database, fixture._root.path(), name);
    }
    fixture
}

pub(super) fn start(fixture: &mut Fixture) -> TurnId {
    let parent = fixture.queued();
    runs::start(&fixture.database.connection, parent).unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        parent
    );
    parent
}

pub(super) fn invoke(
    fixture: &mut Fixture,
    parent: TurnId,
    args: Value,
) -> Result<Invocation, Fault> {
    let invocation = queued(fixture, parent, args)?;
    assert_eq!(
        claim(&mut fixture.database, invocation.turn.id, &fixture.events)?,
        Status::Running
    );
    Ok(invocation)
}

pub(super) fn queued(
    fixture: &mut Fixture,
    parent: TurnId,
    args: Value,
) -> Result<Invocation, Fault> {
    let mut event = AdkEvent::new("delegation-fixture");
    let request = ToolConfirmationRequest {
        tool_name: crate::plugins::tools::alias("delegation", "spawn_agent"),
        function_call_id: Some(format!("call-{}", event.id)),
        args,
    };
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: request.function_call_id.clone(),
            name: request.tool_name.clone(),
            args: request.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        parent,
        event,
        &fixture.events,
    )
    .unwrap();
    admit(
        &mut fixture.database,
        parent,
        &request,
        request.args.clone(),
        None,
        &fixture.events,
    )
}

pub(super) fn finish(fixture: &mut Fixture, turn: TurnId) {
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
}

pub(super) fn profile(fixture: &mut Fixture) -> role::Profile {
    let Output::Role(profile) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            role: role::Profile {
                read_only: false,
                id: RoleId::new(),
                revision: 0,
                key: "review".into(),
                name: "Review".into(),
                description: "Inspect the task".into(),
                instructions: "Frozen review instructions".into(),
                model: None,
                max_turns: Some(5),
                skills: vec![],
                appearance: None,
            },
            expected_revision: 0,
        },
    ) else {
        panic!("role expected")
    };
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: fixture.session,
            expected_revision: 1,
            roles: vec![profile.reference()],
        },
    );
    profile
}

pub(super) fn execute(fixture: &mut Fixture, request: Request) -> Result<Output, Fault> {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, request, reply, &fixture.events)
            .is_none()
    );
    response
        .blocking_recv()
        .unwrap()?
        .completion
        .blocking_recv()
        .unwrap()
}
