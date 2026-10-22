use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Counts {
    acquired: AtomicUsize,
    released: AtomicUsize,
    failures: AtomicUsize,
}
struct Fake {
    counts: Arc<Counts>,
    held: bool,
    fail: bool,
    supported: bool,
}
impl Backend for Fake {
    fn supported(&self) -> bool {
        self.supported
    }
    fn held(&self) -> bool {
        self.held
    }
    fn acquire(&mut self) -> Result<(), i64> {
        self.counts.acquired.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(42)
        } else {
            self.held = true;
            Ok(())
        }
    }
    fn release(&mut self) -> Result<(), i64> {
        self.counts.released.fetch_add(1, Ordering::SeqCst);
        self.held = false;
        Ok(())
    }
}
fn service(fail: bool, supported: bool) -> (Arc<Service>, Arc<Counts>) {
    let counts = Arc::new(Counts::default());
    let failures = counts.clone();
    let service = Service::with_backend(
        Box::new(Fake {
            counts: counts.clone(),
            held: false,
            fail,
            supported,
        }),
        Box::new(move |_| {
            failures.failures.fetch_add(1, Ordering::SeqCst);
        }),
    );
    (service, counts)
}

#[test]
fn enabled_holds_one_request_until_disabled() {
    let (service, counts) = service(false, true);
    service.apply_settings(Settings::default());
    assert!(service.status().enabled);
    assert!(service.status().acquired);
    service.apply_settings(Settings::default());
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 1);
    service.apply_settings(Settings {
        revision: 1,
        prevent_sleep: false,
    });
    assert!(!service.status().enabled);
    assert!(!service.status().acquired);
    assert_eq!(counts.released.load(Ordering::SeqCst), 1);
    service.apply_settings(Settings {
        revision: 2,
        prevent_sleep: true,
    });
    assert!(service.status().acquired);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 2);
}

#[test]
fn restored_disabled_setting_never_acquires() {
    let (service, counts) = service(false, true);
    service.apply_settings(Settings {
        revision: 4,
        prevent_sleep: false,
    });
    assert!(!service.status().enabled);
    assert!(!service.status().acquired);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 0);
}

#[test]
fn concurrent_updates_preserve_latest_revision() {
    let (service, counts) = service(false, true);
    service.apply_settings(Settings {
        revision: 8,
        prevent_sleep: true,
    });
    std::thread::scope(|scope| {
        for revision in 0..8 {
            let service = service.clone();
            scope.spawn(move || {
                service.apply_settings(Settings {
                    revision,
                    prevent_sleep: false,
                })
            });
        }
    });
    assert!(service.status().enabled);
    assert!(service.status().acquired);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 1);
    assert_eq!(counts.released.load(Ordering::SeqCst), 0);
}

#[test]
fn failure_is_reported_once_without_retries() {
    let (service, counts) = service(true, true);
    for revision in 0..8 {
        service.apply_settings(Settings {
            revision,
            prevent_sleep: true,
        });
    }
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 1);
    assert_eq!(counts.failures.load(Ordering::SeqCst), 1);
    assert_eq!(service.status().error, Some(42));
    assert!(!service.status().acquired);
    assert!(service.status().enabled);
}

#[test]
fn close_prevents_late_reactivation() {
    let (service, counts) = service(false, true);
    service.apply_settings(Settings::default());
    service.close();
    service.apply_settings(Settings {
        revision: 9,
        prevent_sleep: true,
    });
    assert!(service.status().closed);
    assert!(!service.status().acquired);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 1);
    assert_eq!(counts.released.load(Ordering::SeqCst), 1);
}

#[test]
fn unsupported_never_claims_an_assertion() {
    let (service, counts) = service(false, false);
    service.apply_settings(Settings::default());
    assert!(!service.status().supported);
    assert!(!service.status().acquired);
    service.apply_settings(Settings {
        revision: 1,
        prevent_sleep: false,
    });
    assert!(!service.status().enabled);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn native_setting_releases() {
    let service = Service::new(|code| panic!("native assertion failed: {code}"));
    service.apply_settings(Settings::default());
    assert!(service.status().acquired);
    service.apply_settings(Settings {
        revision: 1,
        prevent_sleep: false,
    });
    assert!(!service.status().acquired);
    service.apply_settings(Settings {
        revision: 2,
        prevent_sleep: true,
    });
    assert!(service.status().acquired);
    service.close();
    assert!(!service.status().acquired);
}
