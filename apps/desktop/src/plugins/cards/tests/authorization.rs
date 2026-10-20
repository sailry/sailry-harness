use super::*;
use sailry_protocol::{ErrorCode, Fault, RequestOutcome};

#[test]
fn preserves_uncertain_requests_without_replay() {
    for code in [
        sailry_protocol::ErrorCode::OutcomeUnknown,
        sailry_protocol::ErrorCode::Unavailable,
        sailry_protocol::ErrorCode::Internal,
        sailry_protocol::ErrorCode::Busy,
        sailry_protocol::ErrorCode::Cancelled,
    ] {
        assert!(super::super::authorization::uncertain(code));
    }
    assert!(!super::super::authorization::uncertain(
        sailry_protocol::ErrorCode::PermissionDenied
    ));
    let mut login = super::super::authorization::Login::default();
    login.url = Some(("https://example.test/authorize?state=private".into(), 100));
    login.redirect = Some("http://127.0.0.1:43191/callback".into());
    login.suspend();
    assert!(login.url.is_none());
    assert!(login.redirect.is_none());
}

#[test]
fn completed_faults_are_definitive() {
    for code in [
        ErrorCode::Busy,
        ErrorCode::Unavailable,
        ErrorCode::Cancelled,
    ] {
        let fault = Fault::new(code, "isolated completed outcome");
        let (result, confirmed) = super::super::authorization::inspected(Ok(
            RequestOutcome::Completed(Box::new(Err(fault.clone()))),
        ));
        assert!(confirmed);
        assert_eq!(result.unwrap_err(), fault);
    }
    for outcome in [
        RequestOutcome::Unknown,
        RequestOutcome::Admitted,
        RequestOutcome::NotAdmitted,
    ] {
        assert!(!super::super::authorization::inspected(Ok(outcome)).1);
    }
}

#[test]
fn retains_only_live_callback_flows() {
    let mut login = super::super::authorization::Login::default();
    assert!(!login.keeps_alive());
    login.state = host::LoginState::Pending;
    login.url = Some((
        "https://example.test/authorize?state=isolated".into(),
        u64::MAX,
    ));
    assert!(!login.keeps_alive());
    let callback = sailry_client::oauth::Callback::bind("/callback").unwrap();
    login.redirect = Some(callback.redirect.clone());
    login.callback = Some(callback);
    assert!(login.can_open());
    assert!(login.keeps_alive());
    login.url.as_mut().unwrap().1 = 0;
    assert!(!login.can_open());
    assert!(!login.keeps_alive());
    login.suspend();
    assert!(!login.keeps_alive());
}

#[test]
fn resolves_only_the_declared_server_connector() {
    let mut info:plugin::Info=serde_json::from_value(serde_json::json!({
        "summary":{"name":"arbitrary-package","revision":1,"digest":"a".repeat(64),"settings_revision":0,"enabled":true},
        "extension":{"api_version":"v1","connectors":[{
            "id":"service","display":{"label":"Service"},"accounts":"single","origins":["https://example.test"],
            "auth":[{"kind":"mcp_oauth","server":"target"}],"fields":[],"inject":[]
        }]},"skills":[],"mcp":[],"issues":[]
    })).unwrap();
    assert_eq!(
        super::super::authorization::connector_id(&info, "target").unwrap(),
        "service"
    );
    assert_eq!(
        super::super::authorization::connector_id(&info, "other")
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    assert!(crate::plugins::metadata::has_connector(&info, "target"));
    assert!(!crate::plugins::metadata::has_connector(&info, "other"));
    let extension = info.extension.as_mut().unwrap();
    let mut duplicate = extension.connectors[0].clone();
    duplicate.id = "second".into();
    extension.connectors.push(duplicate);
    assert_eq!(
        super::super::authorization::connector_id(&info, "target")
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}
