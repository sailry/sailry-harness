use super::*;
use core::prelude::v1::test;
use sailry_protocol::{Command, Fault};

struct Idle;

impl Transport for Idle {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }
    fn dispatch(
        &self,
        _: Request,
    ) -> sailry_link::Pending<'_, Result<sailry_link::Admission, Fault>> {
        panic!("state tests must not dispatch");
    }
    fn subscribe(
        &self,
        _: sailry_protocol::Topic,
    ) -> sailry_link::Pending<'_, Result<Box<dyn sailry_link::Subscription>, Fault>> {
        panic!("state tests must not subscribe");
    }
}

fn panel() -> Panel {
    Panel {
        binding: Binding {
            client: Arc::new(sailry_client::Client::new(Arc::new(Idle))),
            runtime: Arc::new(
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap(),
            ),
            label: "fixture".into(),
        },
        worktree: WorktreeId::new(),
        active: false,
        root: None,
        sandboxed: false,
        saved: None,
        draft: None,
        conflict: false,
        loading: false,
        pending: false,
        request: None,
        error: None,
    }
}

#[test]
fn pending_drafts() {
    for (scenario, check) in [
        (
            "retains_unsettled_panels",
            (|_scenario: &str| {
                let mut value = panel();
                assert!(value.discardable(), "retains unsettled panels");
                value.apply(Settings::default());
                assert!(value.discardable(), "retains unsettled panels");
                for state in 0..7 {
                    let mut value = panel();
                    value.apply(Settings::default());
                    match state {
                        0 => value.active = true,
                        1 => value.loading = true,
                        2 => value.pending = true,
                        3 => value.conflict = true,
                        4 => value.error = Some("hooks_unknown"),
                        5 => {
                            value.request = Some(Request::new(
                                NodeId([1; 32]),
                                Command::SaveHooks {
                                    worktree: value.worktree,
                                    expected_revision: 0,
                                    settings: Settings::default(),
                                },
                            ))
                        }
                        6 => value.draft.as_mut().unwrap().enabled = true,
                        _ => unreachable!(),
                    }
                    assert!(
                        !value.discardable(),
                        "retains unsettled panels: state {state} must remain available"
                    );
                }
            }) as fn(&str),
        ),
        (
            "waits_for_the_final_saved_draft",
            (|_scenario: &str| {
                let mut value = panel();
                value.apply(Settings::default());
                let first = Settings {
                    enabled: true,
                    ..Default::default()
                };
                value.draft = Some(first.clone());
                let latest = Settings {
                    before_tool: Some(Script {
                        command: "keep this draft".into(),
                        ..Default::default()
                    }),
                    ..first.clone()
                };
                value.draft = Some(latest.clone());
                let first_saved = Settings {
                    revision: 1,
                    ..first.clone()
                };
                assert!(
                    !value.accept(&first, first_saved.clone()),
                    "waits for the final saved draft"
                );
                assert_eq!(
                    value.saved,
                    Some(first_saved),
                    "waits for the final saved draft"
                );
                let latest = Settings {
                    revision: 1,
                    ..latest
                };
                assert_eq!(
                    value.draft,
                    Some(latest.clone()),
                    "waits for the final saved draft"
                );
                assert!(!value.discardable(), "waits for the final saved draft");
                assert!(
                    value.accept(
                        &latest,
                        Settings {
                            revision: 2,
                            ..latest.clone()
                        }
                    ),
                    "waits for the final saved draft"
                );
                assert!(value.discardable(), "waits for the final saved draft");
                let next = Settings {
                    enabled: false,
                    ..value.draft.clone().unwrap()
                };
                value.draft = Some(next.clone());
                assert!(
                    value.accept(
                        &next,
                        Settings {
                            revision: 3,
                            ..next.clone()
                        }
                    ),
                    "waits for the final saved draft"
                );
                assert!(value.discardable(), "waits for the final saved draft");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
