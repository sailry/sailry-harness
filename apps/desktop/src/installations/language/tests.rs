use super::*;
use core::prelude::v1::test;
use sailry_protocol::code::{Install, Installation, Receipt};

fn request() -> Request {
    Request::new(
        NodeId([1; 32]),
        Command::InstallCodeServer {
            install: Install {
                package: "analyzer".into(),
                version: "1".into(),
                purl: "pkg:generic/analyzer@1".into(),
                urls: vec![],
                licenses: vec![],
                command: None,
                revision: "a".repeat(64),
            },
        },
    )
}

fn installed() -> Installation {
    Installation {
        package: "analyzer".into(),
        status: InstallStatus::Ready,
        progress: None,
        error: None,
        receipt: Some(Receipt {
            package: "analyzer".into(),
            version: "1".into(),
            purl: "pkg:generic/analyzer@1".into(),
            revision: "a".repeat(64),
            downloads: vec![],
            installed_at: "2026-10-12T00:00:00Z".into(),
        }),
    }
}

#[test]
fn accepts_only_exact_durable_success() {
    let request = request();
    assert!(completion(&request, &Ok(Output::CodeInstallation(installed()))).is_ok());
    let mut wrong = installed();
    wrong.package = "other".into();
    let mut version = installed();
    version.receipt.as_mut().unwrap().version = "2".into();
    let mut revision = installed();
    revision.receipt.as_mut().unwrap().revision = "b".repeat(64);
    let mut source = installed();
    source.receipt.as_mut().unwrap().purl = "pkg:generic/other@1".into();
    let mut missing = installed();
    missing.receipt = None;
    let mut running = installed();
    running.status = InstallStatus::Installing;
    for output in [wrong, version, revision, source, missing, running] {
        assert_eq!(
            completion(&request, &Ok(Output::CodeInstallation(output)))
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
    }
    assert_eq!(
        completion(
            &request,
            &Ok(Output::InstallationDismissed {
                request: request.id
            })
        )
        .unwrap_err()
        .code,
        ErrorCode::OutcomeUnknown
    );
}

#[test]
fn preserves_confirmed_faults() {
    for code in [
        ErrorCode::Busy,
        ErrorCode::Unavailable,
        ErrorCode::Cancelled,
        ErrorCode::OutcomeUnknown,
    ] {
        let fault = Fault::new(code, "fixture failure");
        assert_eq!(completion(&request(), &Err(fault.clone())), Err(fault));
    }
}
