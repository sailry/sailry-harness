use super::*;
use core::prelude::v1::test;
use sailry_protocol::tool::{
    LocalText,
    detail::{Input, Resolved, Text},
};

fn display(approval: Vec<&str>, summary: Option<&str>, code: bool) -> Resolved {
    Resolved {
        label: Some(LocalText {
            label: "Example".into(),
            locales: Default::default(),
        }),
        input: Input {
            summary: summary.map(|text| Text {
                text: text.into(),
                code,
            }),
            ..Default::default()
        },
        output: None,
        approval: approval
            .into_iter()
            .map(|label| LocalText {
                label: label.into(),
                locales: Default::default(),
            })
            .collect(),
        status: None,
        command: None,
        diagnostics: Vec::new(),
    }
}

fn sample(display: Option<Resolved>) -> (Call, Page) {
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
        failure: None,
        approval: None,
        question: None,
        progress: None,
        resolved: display.clone(),
        detail: None,
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
                presentation: Default::default(),
                grouping: Default::default(),
                summary: Box::new(sailry_protocol::tool::detail::Summary {
                    resolved: display,
                    ..Default::default()
                }),
            }],
            citations: vec![],
            search_suggestions: None,
            usage: None,
        }]
        .into(),
        runs: vec![],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: Vec::new(),
        next_before: None,
    };
    (call, page)
}

#[test]
fn captured_context() {
    for (scenario, check) in [
        (
            "uses_descriptions_without_input_fields",
            (|_scenario: &str| {
                let (call, page) = sample(Some(display(
                    vec!["Create **Task**\n```text\n🙂"],
                    None,
                    false,
                )));
                let details = details(&call, &page);
                assert_eq!(
                    details.description.as_deref(),
                    Some("Create **Task**\n```text\n🙂"),
                    "uses descriptions without input fields"
                );
                assert_eq!(
                    details.prompt, details.description,
                    "uses descriptions without input fields"
                );
                assert_eq!(
                    details.context, None,
                    "uses descriptions without input fields"
                );
            }) as fn(&str),
        ),
        (
            "never_uses_argument_payloads_as_descriptions",
            (|_scenario: &str| {
                for display in [None, Some(display(Vec::new(), None, false))] {
                    let (call, page) = sample(display);
                    assert!(
                        details(&call, &page).description.is_none(),
                        "never uses argument payloads as descriptions"
                    );
                }
                let (call, page) = sample(Some(display(
                    Vec::new(),
                    Some("**Task**\n```text\n🙂"),
                    true,
                )));
                assert_eq!(
                    details(&call, &page).description.as_deref(),
                    Some("**Task**\n```text\n🙂"),
                    "never uses argument payloads as descriptions"
                );
                assert!(
                    details(&call, &page).code,
                    "never uses argument payloads as descriptions"
                );
            }),
        ),
        (
            "omits_missing_optional_notices",
            (|_scenario: &str| {
                let (call, page) = sample(Some(display(
                    vec!["**Task**\n```text\n🙂"],
                    Some("**Task**\n```text\n🙂"),
                    false,
                )));
                let details = details(&call, &page);
                assert_eq!(
                    details.description.as_deref(),
                    Some("**Task**\n```text\n🙂"),
                    "omits missing optional notices"
                );
                assert!(details.notices.is_empty(), "omits missing optional notices");
            }),
        ),
        (
            "uses_complete_context_after_loading",
            (|_scenario: &str| {
                let (call, page) = sample(Some(display(vec!["Bounded"], Some("Bounded"), true)));
                let description = "Canonical command 中文🙂\n".repeat(4000);
                let call = call.with_detail(Arc::new(sailry_protocol::tool::detail::Detail {
                    arguments: serde_json::json!({"command": description}),
                    result: None,
                    resolved: Some(display(vec![&description], Some(&description), true)),
                    content: None,
                    images: Vec::new(),
                }));
                assert_eq!(
                    details(&call, &page).description.as_deref(),
                    Some(description.as_str()),
                    "uses complete context after loading"
                );
            }),
        ),
        (
            "uses_captured_access_without_display",
            (|_scenario: &str| {
                use sailry_protocol::{conversation::ApprovalSource, process::Access};
                let (mut call, page) = sample(None);
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
                    changes: None,
                    can_remember: false,
                    prefix: None,
                    choice: None,
                    reason: None,
                });
                assert_eq!(
                    details(&call, &page).access.as_deref(),
                    Some("Full filesystem and network access"),
                    "uses captured access without display"
                );
                call.approval.as_mut().unwrap().access = Some(Access::Workspace);
                assert!(
                    details(&call, &page).access.is_none(),
                    "uses captured access without display"
                );
            }),
        ),
        (
            "shows_exact_ssh_identity",
            (|_scenario: &str| {
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
                    "Trust ssh.example.invalid:2222 — ssh-ed25519 SHA256:ZhvAsinHmUCtVaKH9IVrCg2YZVkvtDMRpyQD0+dCHQs",
                    "shows exact ssh identity"
                );
                trust.changed = true;
                assert_eq!(
                    ssh_prompt(&trust),
                    "Replace the key for ssh.example.invalid:2222 — ssh-ed25519 SHA256:ZhvAsinHmUCtVaKH9IVrCg2YZVkvtDMRpyQD0+dCHQs",
                    "shows exact ssh identity"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
