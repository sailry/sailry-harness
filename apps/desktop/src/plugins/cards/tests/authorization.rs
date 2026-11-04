use super::*;
use sailry_protocol::{ErrorCode, Fault, RequestOutcome};

#[test]
fn captured_flows() {
    for (scenario, check) in [
        (
            "preserves_uncertain_requests_without_replay",
            (|_scenario: &str| {
                for code in [
                    sailry_protocol::ErrorCode::OutcomeUnknown,
                    sailry_protocol::ErrorCode::Unavailable,
                    sailry_protocol::ErrorCode::Internal,
                    sailry_protocol::ErrorCode::Busy,
                    sailry_protocol::ErrorCode::Cancelled,
                ] {
                    assert!(
                        super::super::authorization::uncertain(code),
                        "preserves uncertain requests without replay"
                    );
                }
                assert!(
                    !super::super::authorization::uncertain(
                        sailry_protocol::ErrorCode::PermissionDenied
                    ),
                    "preserves uncertain requests without replay"
                );
                let mut login = super::super::authorization::Login::default();
                login.url = Some(("https://example.test/authorize?state=private".into(), 100));
                login.redirect = Some("http://127.0.0.1:43191/callback".into());
                login.suspend();
                assert!(
                    login.url.is_none(),
                    "preserves uncertain requests without replay"
                );
                assert!(
                    login.redirect.is_none(),
                    "preserves uncertain requests without replay"
                );
            }) as fn(&str),
        ),
        (
            "completed_faults_are_definitive",
            (|_scenario: &str| {
                for code in [
                    ErrorCode::Busy,
                    ErrorCode::Unavailable,
                    ErrorCode::Cancelled,
                ] {
                    let fault = Fault::new(code, "isolated completed outcome");
                    let (result, confirmed) = super::super::authorization::inspected(Ok(
                        RequestOutcome::Completed(Box::new(Err(fault.clone()))),
                    ));
                    assert!(confirmed, "completed faults are definitive");
                    assert_eq!(
                        result.unwrap_err(),
                        fault,
                        "completed faults are definitive"
                    );
                }
                for outcome in [
                    RequestOutcome::Unknown,
                    RequestOutcome::Admitted,
                    RequestOutcome::NotAdmitted,
                ] {
                    assert!(
                        !super::super::authorization::inspected(Ok(outcome)).1,
                        "completed faults are definitive"
                    );
                }
            }),
        ),
        (
            "retains_only_live_callback_flows",
            (|_scenario: &str| {
                let mut login = super::super::authorization::Login::default();
                assert!(!login.keeps_alive(), "retains only live callback flows");
                login.state = host::LoginState::Pending;
                login.url = Some((
                    "https://example.test/authorize?state=isolated".into(),
                    u64::MAX,
                ));
                assert!(!login.keeps_alive(), "retains only live callback flows");
                let callback = sailry_client::oauth::Callback::bind("/callback").unwrap();
                login.redirect = Some(callback.redirect.clone());
                login.callback = Some(callback);
                assert!(login.can_open(), "retains only live callback flows");
                assert!(login.keeps_alive(), "retains only live callback flows");
                login.url.as_mut().unwrap().1 = 0;
                assert!(!login.can_open(), "retains only live callback flows");
                assert!(!login.keeps_alive(), "retains only live callback flows");
                login.suspend();
                assert!(!login.keeps_alive(), "retains only live callback flows");
            }),
        ),
        (
            "resolves_only_the_declared_server_connector",
            (|_scenario: &str| {
                let mut info:plugin::Info=serde_json::from_value(serde_json::json!({
        "summary":{"name":"arbitrary-package","revision":1,"digest":"a".repeat(64),"settings_revision":0,"enabled":true},
        "extension":{"api_version":"v1","connectors":[{
            "id":"service","display":{"label":"Service"},"accounts":"single","origins":["https://example.test"],
            "auth":[{"kind":"mcp_oauth","server":"target"}],"fields":[],"inject":[]
                }]},"skills":[],"mcp":[],"issues":[]
            })).unwrap();
                assert_eq!(
                    super::super::authorization::connector_id(&info, "target").unwrap(),
                    "service",
                    "resolves only the declared server connector"
                );
                assert_eq!(
                    super::super::authorization::connector_id(&info, "other")
                        .unwrap_err()
                        .code,
                    ErrorCode::NotConfigured,
                    "resolves only the declared server connector"
                );
                assert!(
                    crate::plugins::metadata::has_connector(&info, "target"),
                    "resolves only the declared server connector"
                );
                assert!(
                    !crate::plugins::metadata::has_connector(&info, "other"),
                    "resolves only the declared server connector"
                );
                let extension = info.extension.as_mut().unwrap();
                let mut duplicate = extension.connectors[0].clone();
                duplicate.id = "second".into();
                extension.connectors.push(duplicate);
                assert_eq!(
                    super::super::authorization::connector_id(&info, "target")
                        .unwrap_err()
                        .code,
                    ErrorCode::InvalidRequest,
                    "resolves only the declared server connector"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
