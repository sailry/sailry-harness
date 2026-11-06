use super::*;
use sailry_protocol::Command;

#[test]
fn accepts_exact_distribution_names() {
    for name in ["Ubuntu", "Ubuntu 资料", "Distro ' $; &"] {
        assert!(validate(name).is_ok());
    }
    for name in [
        "",
        "--install",
        "Ubuntu\nDebian",
        "Ubuntu\0Debian",
        &"x".repeat(257),
    ] {
        assert!(validate(name).is_err());
    }
}

#[test]
fn scopes_progress_and_cancellation() {
    let service = Arc::new(Service::default());
    let caller = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let request = Request::new(
        caller,
        Command::InstallWslHost {
            distribution: "Ubuntu 资料".into(),
            network: None,
        },
    );
    let closed = CancellationToken::new();
    let installation = service.begin(caller, &request, &closed).unwrap();
    assert_eq!(
        service.progress(caller, request.id, &None).unwrap(),
        Output::HostInstallProgress(InstallProgress::Connecting)
    );
    assert_eq!(
        service.progress(other, request.id, &None).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(service.cancel(other, request.id, &None).is_ok());
    assert!(!installation.stop.is_cancelled());
    assert_eq!(
        service.begin(other, &request, &closed).err().unwrap().code,
        ErrorCode::Conflict
    );
    installation
        .progress
        .send_replace(InstallProgress::Installing);
    assert_eq!(
        service.progress(caller, request.id, &None).unwrap(),
        Output::HostInstallProgress(InstallProgress::Installing)
    );
    service.cancel(caller, request.id, &None).unwrap();
    assert!(installation.stop.is_cancelled());
    drop(installation);
    assert_eq!(
        service
            .progress(caller, request.id, &None)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let installation = service.begin(caller, &request, &closed).unwrap();
    closed.cancel();
    assert!(installation.stop.is_cancelled());
}

#[test]
fn preserves_plugin_scope() {
    let service = Arc::new(Service::default());
    let caller = NodeId([1; 32]);
    let mut request = Request::new(
        caller,
        Command::InstallWslHost {
            distribution: "Ubuntu".into(),
            network: None,
        },
    );
    let context = |name: &str| plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: plugin::Reference {
            name: name.into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        worktree: None,
        session: None,
    };
    request.plugin = Some(context("fixture"));
    let installation = service
        .begin(caller, &request, &CancellationToken::new())
        .unwrap();
    let other = Some(context("other"));
    assert_eq!(
        service
            .progress(caller, request.id, &other)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        service.cancel(caller, request.id, &other).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert!(!installation.stop.is_cancelled());
}
