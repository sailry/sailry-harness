//! Keep fixture failures inside Rust before returning to the native event loop.
use futures::FutureExt;
use std::{
    any::Any,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicBool, Ordering},
};

static FAILED: AtomicBool = AtomicBool::new(false);

pub(crate) fn install_exit() {
    // AppKit terminates the process after GPUI shutdown instead of returning from run.
    #[cfg(target_os = "macos")]
    if unsafe { libc::atexit(exit_status) } != 0 {
        eprintln!("Cannot register native acceptance exit status");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
extern "C" fn exit_status() {
    if failed() {
        // Avoid recursively entering exit while preserving the failed test status.
        unsafe { libc::_exit(1) };
    }
}

pub(crate) fn failed() -> bool {
    FAILED.load(Ordering::Relaxed)
}

pub(crate) fn fail(error: impl std::fmt::Display) {
    FAILED.store(true, Ordering::Relaxed);
    eprintln!("Native acceptance failed: {error}");
}

pub(crate) async fn check<T>(task: impl Future<Output = T>) -> Option<T> {
    outcome(AssertUnwindSafe(task).catch_unwind().await)
}

pub(crate) fn setup<T>(task: impl FnOnce() -> T) -> Option<T> {
    outcome(catch_unwind(AssertUnwindSafe(task)))
}

fn outcome<T>(result: std::thread::Result<T>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            fail(message(error.as_ref()));
            None
        }
    }
}

fn message(error: &(dyn Any + Send)) -> &str {
    error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("fixture panicked without a text diagnostic")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_success() {
        assert_eq!(futures::executor::block_on(check(async { 42 })), Some(42));
        assert_eq!(setup(|| 42), Some(42));
    }

    #[test]
    fn contains_setup_failure() {
        assert_eq!(setup(|| panic!("fixture setup")), None::<()>);
        assert!(failed());
    }

    #[test]
    fn contains_poll_failure() {
        let result = futures::executor::block_on(check(async {
            futures::future::ready(()).await;
            panic!("fixture deadline");
        }));
        assert_eq!(result, None::<()>);
        assert!(failed());
    }

    #[test]
    fn contains_worker_failure() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(check(async {
            let worker = tokio::spawn(async {
                panic!("terminal fixture deadline");
            });
            worker.await.expect("native worker must complete");
        }));
        assert_eq!(result, None::<()>);
        assert!(failed());
    }

    #[test]
    fn retains_owned_diagnostic() {
        let error: Box<dyn Any + Send> = Box::new(String::from("worker failed"));
        assert_eq!(message(error.as_ref()), "worker failed");
    }
}
