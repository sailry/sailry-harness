use super::*;

const ENVIRONMENT: &str = "Session environment";
const SKILL: &str = "Retained owner context: skill:fixture:SKILL.md\nBody";
const PLAN: &str = "Retained owner context: plan\nSteps";
const MEMORY: &str = "Relevant memory: {\"id\":\"one\"}";
const OTHER: &str = "Relevant memory: {\"id\":\"two\"}";

fn persisted(parts: &[&str]) -> Message {
    message(
        &parts
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>(),
        &[SKILL, PLAN, MEMORY, OTHER],
    )
    .unwrap()
}

fn select(messages: Vec<Message>, parts: &[&str]) -> Vec<String> {
    pending(
        &messages,
        &[SKILL, PLAN, MEMORY, OTHER],
        parts.iter().map(|part| (*part).to_owned()).collect(),
    )
}

#[test]
fn starts_with_every_part() {
    assert_eq!(
        select(
            vec![Message::user().with_text("Task")],
            &[ENVIRONMENT, SKILL]
        ),
        [ENVIRONMENT, SKILL]
    );
    assert!(select(vec![Message::user().with_text("Task")], &[]).is_empty());
}

#[test]
fn skips_unchanged_turn() {
    let history = vec![
        Message::user().with_text("Task"),
        persisted(&[ENVIRONMENT, SKILL]),
        Message::assistant().with_text("Working"),
    ];
    assert!(select(history.clone(), &[ENVIRONMENT, SKILL]).is_empty());
    assert_eq!(
        select(history.clone(), &["Moved environment", SKILL]),
        ["Moved environment"]
    );
    assert_eq!(
        select(history, &[ENVIRONMENT, SKILL, PLAN]),
        [ENVIRONMENT, PLAN]
    );
}

#[test]
fn states_each_turn_once() {
    let history = vec![
        Message::user().with_text("Task"),
        persisted(&[ENVIRONMENT, SKILL]),
        Message::assistant().with_text("Done"),
        Message::user().with_text("Next task"),
    ];
    assert_eq!(select(history, &[ENVIRONMENT, SKILL]), [ENVIRONMENT]);
}

#[test]
fn keeps_state_beside_owner_parts() {
    let history = vec![
        Message::user().with_text("Task"),
        persisted(&[ENVIRONMENT, SKILL]),
        persisted(&[ENVIRONMENT, PLAN]),
    ];
    assert!(select(history, &[ENVIRONMENT, SKILL, PLAN]).is_empty());
    let owners = vec![Message::user().with_text("Task"), persisted(&[SKILL])];
    assert!(select(owners.clone(), &[SKILL]).is_empty());
    assert_eq!(select(owners, &[ENVIRONMENT, SKILL]), [ENVIRONMENT]);
}

#[test]
fn states_plugin_parts_once_per_conversation() {
    let history = vec![
        Message::user().with_text("Task"),
        persisted(&[ENVIRONMENT, MEMORY]),
        Message::assistant().with_text("Done"),
        Message::user().with_text("Next task"),
    ];
    assert_eq!(
        select(history.clone(), &[ENVIRONMENT, MEMORY]),
        [ENVIRONMENT]
    );
    assert_eq!(
        select(history.clone(), &[ENVIRONMENT, MEMORY, OTHER]),
        [ENVIRONMENT, OTHER]
    );
    assert_eq!(
        select(history, &[ENVIRONMENT, OTHER, MEMORY]),
        [ENVIRONMENT, OTHER]
    );
}

#[test]
fn keeps_state_beside_plugin_parts() {
    let history = vec![
        Message::user().with_text("Task"),
        persisted(&[ENVIRONMENT, MEMORY]),
    ];
    assert!(select(history.clone(), &[ENVIRONMENT, MEMORY]).is_empty());
    assert_eq!(
        select(history.clone(), &[ENVIRONMENT, MEMORY, OTHER]),
        [ENVIRONMENT, OTHER]
    );
    assert_eq!(
        select(history, &["Moved environment", MEMORY]),
        ["Moved environment"]
    );
}

#[test]
fn restores_hidden_plugin_parts() {
    let mut hidden = persisted(&[ENVIRONMENT, MEMORY]);
    hidden.metadata.agent_visible = false;
    assert_eq!(
        select(
            vec![Message::user().with_text("Task"), hidden],
            &[ENVIRONMENT, MEMORY]
        ),
        [ENVIRONMENT, MEMORY]
    );
}

#[test]
fn restores_hidden_owner_parts() {
    let mut hidden = persisted(&[ENVIRONMENT, SKILL]);
    hidden.metadata.agent_visible = false;
    assert_eq!(
        select(
            vec![Message::user().with_text("Task"), hidden],
            &[ENVIRONMENT, SKILL]
        ),
        [ENVIRONMENT, SKILL]
    );
}

#[test]
fn keeps_distinct_substrings() {
    for (previous, current) in [
        ("Do not deploy on Fridays", "Do not deploy"),
        ("Do not deploy to production", "deploy to production"),
        ("First paragraph\n\nSecond paragraph", "Second paragraph"),
    ] {
        let history = vec![
            Message::user().with_text("Task"),
            message(&[ENVIRONMENT.into(), previous.into()], &[previous]).unwrap(),
            Message::assistant().with_text("Done"),
            Message::user().with_text("Next task"),
        ];
        let parts = pending(
            &history,
            &[current],
            vec![ENVIRONMENT.into(), current.into()],
        );
        assert_eq!(parts, [ENVIRONMENT, current]);
    }
}

#[test]
fn ignores_state_substrings() {
    let part = "Do not deploy";
    let history = vec![
        Message::user().with_text("Task"),
        message(&["Environment\nDo not deploy on Fridays".into()], &[]).unwrap(),
    ];
    assert_eq!(
        pending(&history, &[part], vec![ENVIRONMENT.into(), part.into()]),
        [ENVIRONMENT, part]
    );
}

#[test]
fn compares_state_without_prefix_matching() {
    let part = "Do not deploy";
    let previous = "Environment\nDo not deploy in staging";
    let current = "Environment\nDo not deploy in production";
    let history = vec![
        Message::user().with_text("Task"),
        message(&[previous.into(), part.into()], &[part]).unwrap(),
    ];
    assert_eq!(
        pending(&history, &[part], vec![current.into(), part.into()]),
        [current]
    );
}

#[test]
fn requires_persisted_identity() {
    let mut earlier = persisted(&[ENVIRONMENT, MEMORY]);
    earlier.metadata.operations = None;
    let mut history = vec![Message::user().with_text("Task"), earlier];
    let selected = pending(&history, &[MEMORY], vec![ENVIRONMENT.into(), MEMORY.into()]);
    assert_eq!(selected, [ENVIRONMENT, MEMORY]);
    history.push(message(&selected, &[MEMORY]).unwrap());
    assert!(select(history, &[ENVIRONMENT, MEMORY]).is_empty());
}

#[test]
fn compares_state_without_owned_parts() {
    let mut history = vec![
        Message::user().with_text("Task"),
        message(&[ENVIRONMENT.into()], &[]).unwrap(),
    ];
    assert!(pending(&history, &[], vec![ENVIRONMENT.into()]).is_empty());
    assert_eq!(
        pending(&history, &[], vec!["Moved environment".into()]),
        ["Moved environment"]
    );
    history.push(Message::assistant().with_text("Done"));
    history.push(Message::user().with_text("Next task"));
    assert_eq!(
        pending(&history, &[], vec![ENVIRONMENT.into()]),
        [ENVIRONMENT]
    );
}

#[test]
fn keeps_first_occurrence() {
    let history = vec![Message::user().with_text("Task")];
    assert_eq!(
        pending(
            &history,
            &[MEMORY, OTHER],
            vec![
                ENVIRONMENT.into(),
                OTHER.into(),
                MEMORY.into(),
                OTHER.into()
            ]
        ),
        [ENVIRONMENT, OTHER, MEMORY]
    );
}

#[test]
fn restores_only_hidden_parts() {
    let mut original = persisted(&[ENVIRONMENT, MEMORY]);
    original.metadata.agent_visible = false;
    let history = vec![
        Message::user().with_text("Task"),
        original,
        persisted(&[ENVIRONMENT, OTHER]),
    ];
    assert_eq!(
        select(history, &[ENVIRONMENT, MEMORY, OTHER]),
        [ENVIRONMENT, MEMORY]
    );
}
