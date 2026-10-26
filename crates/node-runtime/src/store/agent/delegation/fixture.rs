use super::*;
use crate::store::agent::tests::command;
pub(super) use crate::store::agent::tests::fixture::Fixture;

pub(super) fn setup() -> sailry_link::Pending<'static, Fixture> {
    Box::pin(async move {
        let fixture = Fixture::new().await;
        for name in ["delegation", "files"] {
            crate::store::agent::tests::fixture::package(
                &fixture.database,
                fixture._root.path(),
                name,
            )
            .await;
        }
        fixture
    })
}

pub(super) fn start<'a>(fixture: &'a mut Fixture) -> sailry_link::Pending<'a, TurnId> {
    Box::pin(async move {
        let parent = fixture.queued().await;
        runs::start(&fixture.database.connection, parent)
            .await
            .unwrap();
        assert_eq!(
            runs::claim(&mut fixture.database, &fixture.events)
                .await
                .unwrap()
                .unwrap()
                .turn
                .id,
            parent
        );
        parent
    })
}

pub(super) fn invoke<'a>(
    fixture: &'a mut Fixture,
    parent: TurnId,
    args: Value,
) -> sailry_link::Pending<'a, Result<Invocation, Fault>> {
    Box::pin(async move {
        let invocation = queued(fixture, parent, args).await?;
        assert_eq!(
            claim(&mut fixture.database, invocation.turn.id, &fixture.events).await?,
            Status::Running
        );
        Ok(invocation)
    })
}

pub(super) fn queued<'a>(
    fixture: &'a mut Fixture,
    parent: TurnId,
    args: Value,
) -> sailry_link::Pending<'a, Result<Invocation, Fault>> {
    Box::pin(async move {
        let mut event = AdkEvent::new("delegation-fixture");
        let request = ToolConfirmationRequest {
            acting: None,
            origin: None,
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
        .await
        .unwrap();
        admit(
            &mut fixture.database,
            parent,
            &request,
            request.args.clone(),
            None,
            &fixture.events,
        )
        .await
    })
}

pub(super) fn finish<'a>(fixture: &'a mut Fixture, turn: TurnId) -> sailry_link::Pending<'a, ()> {
    Box::pin(async move {
        runs::finish(
            &mut fixture.database,
            turn,
            Status::Completed,
            None,
            &fixture.events,
        )
        .await
        .unwrap();
    })
}

pub(super) fn profile<'a>(fixture: &'a mut Fixture) -> sailry_link::Pending<'a, role::Profile> {
    Box::pin(async move {
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
                    tools: None,
                    appearance: None,
                },
                expected_revision: 0,
            },
        )
        .await
        else {
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
        )
        .await;
        profile
    })
}

pub(super) fn execute<'a>(
    fixture: &'a mut Fixture,
    request: Request,
) -> sailry_link::Pending<'a, Result<Output, Fault>> {
    Box::pin(async move {
        let (reply, response) = oneshot::channel();
        assert!(
            fixture
                .database
                .dispatch(fixture.database.node, request, reply, &fixture.events)
                .await
                .is_none()
        );
        response.await.unwrap()?.completion.await.unwrap()
    })
}
