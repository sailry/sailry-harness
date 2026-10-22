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
fn concurrent_leases_share_one_request() {
    let (service, counts) = service(false, true);
    let barrier = Arc::new(std::sync::Barrier::new(9));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let (service, barrier) = (service.clone(), barrier.clone());
            scope.spawn(move || {
                let _lease = service.acquire();
                barrier.wait();
                barrier.wait();
            });
        }
        barrier.wait();
        assert_eq!(service.status().active, 8);
        assert!(service.status().acquired);
        assert_eq!(counts.acquired.load(Ordering::SeqCst), 1);
        barrier.wait();
    });
    assert_eq!(service.status().active, 0);
    assert!(!service.status().acquired);
    assert_eq!(counts.released.load(Ordering::SeqCst), 1);
}

#[test]
fn toggles_reconcile_active_work() {
    let (service, counts) = service(false, true);
    let lease = service.acquire();
    service.apply_settings(Settings {
        revision: 1,
        prevent_sleep: false,
    });
    assert_eq!(service.status().active, 1);
    assert!(!service.status().acquired);
    service.apply_settings(Settings {
        revision: 2,
        prevent_sleep: true,
    });
    assert!(service.status().acquired);
    service.apply_settings(Settings {
        revision: 1,
        prevent_sleep: false,
    });
    assert!(service.status().enabled);
    drop(lease);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 2);
    assert_eq!(counts.released.load(Ordering::SeqCst), 2);
}

#[test]
fn failure_is_reported_once_without_retries() {
    let (service, counts) = service(true, true);
    for _ in 0..8 {
        drop(service.acquire());
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
    let lease = service.acquire();
    service.close();
    let late = service.acquire();
    service.apply_settings(Settings {
        revision: 9,
        prevent_sleep: true,
    });
    assert!(service.status().closed);
    assert!(!service.status().acquired);
    drop((late, lease));
    assert_eq!(service.status().active, 0);
    assert_eq!(counts.acquired.load(Ordering::SeqCst), 1);
    assert_eq!(counts.released.load(Ordering::SeqCst), 1);
}

#[test]
fn unsupported_never_claims_an_assertion() {
    let (service, counts) = service(false, false);
    let _lease = service.acquire();
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
fn native_assertion_releases() {
    let service = Service::new(|code| panic!("native assertion failed: {code}"));
    let lease = service.acquire();
    assert!(service.status().acquired);
    drop(lease);
    assert!(!service.status().acquired);
    service.close();
}
