use super::{Notice, Service, State, Update};
use crate::updater::{
    Failure,
    tests::fixture,
    transfer::{Progress, Staged},
};
use gpui_kit::TestAppContext;
use sailry_link::CancellationToken;
use std::{cell::RefCell, rc::Rc};

#[gpui_kit::test]
fn cancels_and_ignores_stale_results(cx: &mut TestAppContext) {
    cx.update(crate::preferences::init);
    let service = cx.update(Service::shared);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let observed = notices.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&service, move |_, notice: &Notice, _| {
            observed.borrow_mut().push(notice.clone());
        })
    });
    cx.update(|cx| {
        service.update(cx, |service, cx| {
            let config = fixture::config();
            let manifest = fixture::signed(&[fixture::release()]);
            let selection = super::super::manifest::select(&manifest, &config)
                .unwrap()
                .unwrap();

            service.state = State::Checking;
            service.stop = CancellationToken::new();
            let stopped = service.stop.clone();
            let stale_check = service.advance();
            assert!(service.channel_changed(cx));
            assert!(stopped.is_cancelled());
            service.accept(
                stale_check,
                Update::Checked(Ok((config.clone(), Some(selection.clone())))),
                cx,
            );
            assert_eq!(service.state, State::Idle);
            assert!(service.config.is_none());
            assert!(service.selection.is_none());

            service.state = State::Downloading(Progress::Downloading {
                copied: 1,
                total: Some(2),
            });
            service.config = Some(config);
            service.selection = Some(selection.clone());
            service.stop = CancellationToken::new();
            let stopped = service.stop.clone();
            let stale_download = service.advance();
            let directory = tempfile::tempdir().unwrap();
            let archive = directory.path().join(&selection.release.name);
            let staged = Staged {
                directory,
                archive,
                selection,
            };
            assert!(service.channel_changed(cx));
            assert!(stopped.is_cancelled());
            service.accept(stale_download, Update::Downloaded(Ok(staged)), cx);
            assert_eq!(service.state, State::Idle);
            assert!(service.staged.is_none());
        });
    });
    assert!(notices.borrow().is_empty());
}

#[gpui_kit::test]
fn announces_download_without_panel(cx: &mut TestAppContext) {
    let service = cx.update(Service::shared);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let observed = notices.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&service, move |_, notice: &Notice, _| {
            observed.borrow_mut().push(notice.clone());
        })
    });
    service.update(cx, |service, cx| {
        let config = fixture::config();
        let manifest = fixture::signed(&[fixture::release()]);
        let selection = super::super::manifest::select(&manifest, &config)
            .unwrap()
            .unwrap();
        let version = selection.release.version.clone();
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join(&selection.release.name);
        service.accept(
            service.operation,
            Update::Downloaded(Ok(Staged {
                directory,
                archive,
                selection,
            })),
            cx,
        );
        assert_eq!(service.state, State::Ready(version));
        assert!(service.staged.is_some());
    });
    assert_eq!(
        *notices.borrow(),
        [Notice {
            key: "updates_ready",
            error: false,
            version: Some("9.9.9".into()),
        }],
    );
}

#[gpui_kit::test]
fn reports_failures_once_without_panel(cx: &mut TestAppContext) {
    let service = cx.update(Service::shared);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let observed = notices.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&service, move |_, notice: &Notice, _| {
            observed.borrow_mut().push(notice.clone());
        })
    });
    for downloaded in [false, true] {
        let failure = Failure::new("updates_network_failed", "isolated failure detail");
        service.update(cx, |service, cx| {
            let update = if downloaded {
                Update::Downloaded(Err(failure.clone()))
            } else {
                Update::Checked(Err(failure.clone()))
            };
            service.accept(service.operation, update, cx);
            assert_eq!(service.failure, Some(failure));
        });
        assert_eq!(
            std::mem::take(&mut *notices.borrow_mut()),
            [Notice {
                key: "updates_network_failed",
                error: true,
                version: None,
            }],
        );
    }
}

#[gpui_kit::test]
fn cancellation_has_no_error_notice(cx: &mut TestAppContext) {
    let service = cx.update(Service::shared);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let observed = notices.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&service, move |_, notice: &Notice, _| {
            observed.borrow_mut().push(notice.clone());
        })
    });
    service.update(cx, |service, cx| {
        service.accept(
            service.operation,
            Update::Downloaded(Err(Failure::new("updates_cancelled", "cancelled"))),
            cx,
        );
        assert_eq!(service.state, State::Idle);
        assert!(service.failure.is_none());
    });
    assert!(notices.borrow().is_empty());
}

#[gpui_kit::test]
fn retains_receipt_failure(cx: &mut TestAppContext) {
    let service = cx.update(Service::shared);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let observed = notices.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&service, move |_, notice: &Notice, _| {
            observed.borrow_mut().push(notice.clone());
        })
    });
    service.update(cx, |service, cx| {
        let failure = Failure::new("updates_restart_failed", "isolated restart detail");
        service.receipt(Err(failure.clone()), cx);
        assert_eq!(service.failure, Some(failure));
    });
    assert_eq!(
        *notices.borrow(),
        [Notice {
            key: "updates_restart_failed",
            error: true,
            version: None,
        }],
    );
}

#[gpui_kit::test]
fn reads_startup_receipt_once(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let updater = directory.path().join("updater");
    std::fs::create_dir(&updater).unwrap();
    let receipt = updater.join("receipt.json");
    let outcome = serde_json::json!({
        "version": 1,
        "result": "success",
        "application_version": env!("CARGO_PKG_VERSION"),
    });
    super::super::receipt::write(&receipt, &outcome).unwrap();
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(
            directory.path().join("preferences.json"),
        ));
        super::init(cx);
    });
    let service = cx.update(Service::shared);
    let notices = Rc::new(RefCell::new(Vec::new()));
    let observed = notices.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&service, move |_, notice: &Notice, _| {
            observed.borrow_mut().push(notice.clone());
        })
    });
    cx.update(super::startup);
    cx.run_until_parked();
    assert!(!receipt.exists());
    assert_eq!(
        std::fs::read_dir(updater.join("receipts")).unwrap().count(),
        1
    );
    super::super::receipt::write(&receipt, &outcome).unwrap();
    cx.update(super::startup);
    cx.run_until_parked();
    assert!(receipt.exists());
    assert_eq!(
        *notices.borrow(),
        [Notice {
            key: "updates_completed",
            error: false,
            version: None,
        }],
    );
}
