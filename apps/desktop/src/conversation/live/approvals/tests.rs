use super::*;
use core::prelude::v1::test;
use serde_json::json;

fn sample(display: Option<serde_json::Value>) -> (Call, Page) {
    let turn = sailry_protocol::TurnId::new();
    let call = Call {
        terminal_interaction: false,
        turn,
        id: Some("call".into()),
        name: "example".into(),
        presentation: Default::default(),
        grouping: Default::default(),
        source: sailry_client::conversation::tools::Reference {
            entry: "entry".into(),
            index: 0,
        },
        response: None,
        state: sailry_client::conversation::tools::State::Waiting,
        approval: None,
        question: None,
        progress: None,
        content: None,
        resolved: None,
    };
    let page = Page {
        session: sailry_protocol::SessionId::new(),
        revision: 1,
        entries: vec![sailry_protocol::conversation::Entry {
            sequence: 1,
            id: "entry".into(),
            turn,
            author: "assistant".into(),
            branch: String::new(),
            timestamp_ms: 1,
            parts: vec![sailry_protocol::conversation::Part::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: json!({"name":"**Task**\n```text\n🙂", "text":"Unrelated write body"}),
                presentation: Default::default(),
                grouping: Default::default(),
                display: display.map(|display| serde_json::from_value(display).unwrap()),
            }],
            citations: vec![],
            search_suggestions: None,
            usage: None,
        }],
        runs: vec![],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        next_before: None,
    };
    (call, page)
}

#[test]
fn uses_descriptions_without_input_fields() {
    let (call, page) = sample(Some(json!({
        "label":"Example", "approval":[{
            "message":{"label":"Create %{name}"},
            "values":{"name":{"source":"arguments", "path":"/name"}},
        }],
    })));
    let details = details(&call, &page);
    assert_eq!(
        details.description.as_deref(),
        Some("Create **Task**\n```text\n🙂")
    );
    assert_eq!(details.prompt, details.description);
    assert_eq!(details.context, None);
}

#[test]
fn never_uses_argument_payloads_as_descriptions() {
    for display in [None, Some(json!({"label":"Example"}))] {
        let (call, page) = sample(display);
        assert!(details(&call, &page).description.is_none());
    }
    let (call, page) = sample(Some(json!({
        "label":"Example", "input":{
            "summary":{"path":"/name", "code":true},
            "content":{"path":"/text"},
        },
    })));
    assert_eq!(
        details(&call, &page).description.as_deref(),
        Some("**Task**\n```text\n🙂")
    );
    assert!(details(&call, &page).code);
}

#[test]
fn omits_missing_optional_notices() {
    let (call, page) = sample(Some(json!({
        "label":"Command", "input":{"summary":{"path":"/name"}},
        "approval":[
            {"message":{"label":"%{command}"},"values":{"command":{"source":"arguments","path":"/name"}}},
            {"message":{"label":"%{description}"},"values":{"description":{"source":"arguments","path":"/description"}},
             "when":{"all":[{"value":{"source":"arguments","path":"/description"},"equals":null,"negate":true}]}},
        ],
    })));
    let details = details(&call, &page);
    assert_eq!(
        details.description.as_deref(),
        Some("**Task**\n```text\n🙂")
    );
    assert!(details.notices.is_empty());
}

#[test]
fn uses_captured_access_without_display() {
    use sailry_protocol::{conversation::ApprovalSource, process::Access};
    let (mut call, mut page) = sample(None);
    call.approval = Some(Approval {
        id: ApprovalId::new(),
        session: page.session,
        turn: call.turn,
        entry: call.source.entry.clone(),
        index: call.source.index,
        state: ApprovalState::Pending,
        source: ApprovalSource::User,
        access: Some(Access::Unrestricted),
        startup: None,
        ssh: None,
        can_remember: false,
        prefix: None,
        choice: None,
        reason: None,
    });
    let sailry_protocol::conversation::Part::ToolCall { arguments, .. } =
        &mut page.entries[0].parts[0]
    else {
        unreachable!()
    };
    arguments["access"] = json!("workspace");
    assert_eq!(
        details(&call, &page).access.as_deref(),
        Some("Full filesystem and network access")
    );
    call.approval.as_mut().unwrap().access = Some(Access::Workspace);
    let sailry_protocol::conversation::Part::ToolCall { arguments, .. } =
        &mut page.entries[0].parts[0]
    else {
        unreachable!()
    };
    arguments["access"] = json!("unrestricted");
    assert!(details(&call, &page).access.is_none());
}

#[test]
fn shows_exact_ssh_identity() {
    let mut trust = sailry_protocol::ssh::Trust {
        profile: sailry_protocol::SshId::new(),
        expected_revision: 7,
        host: "ssh.example.invalid".into(),
        port: 2222,
        key: sailry_protocol::ssh::HostKey {
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:ZhvAsinHmUCtVaKH9IVrCg2YZVkvtDMRpyQD0+dCHQs".into(),
        },
        changed: false,
    };
    assert_eq!(
        ssh_prompt(&trust),
        "Trust ssh.example.invalid:2222 — ssh-ed25519 SHA256:ZhvAsinHmUCtVaKH9IVrCg2YZVkvtDMRpyQD0+dCHQs"
    );
    trust.changed = true;
    assert_eq!(
        ssh_prompt(&trust),
        "Replace the key for ssh.example.invalid:2222 — ssh-ed25519 SHA256:ZhvAsinHmUCtVaKH9IVrCg2YZVkvtDMRpyQD0+dCHQs"
    );
}
