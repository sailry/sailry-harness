//! Bounded CPU inference sessions shared by local model algorithms.
use ort::{
    environment::ThreadManager,
    session::{Session, builder::SessionBuilder},
};
use std::thread::JoinHandle;

pub fn threads() -> usize {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    (available / 2).clamp(1, 4)
}

/// Runtime initialization and model validation remain explicit at each loader.
pub fn builder() -> ort::Result<SessionBuilder> {
    Session::builder()?
        .with_independent_thread_pool()
        .map_err(ort::Error::<()>::from)?
        .with_no_environment_execution_providers()
        .map_err(ort::Error::<()>::from)?
        .with_execution_providers([ort::ep::CPU::default().build()])
        .map_err(ort::Error::<()>::from)?
        .with_parallel_execution(false)
        .map_err(ort::Error::<()>::from)?
        .with_intra_threads(threads())
        .map_err(ort::Error::<()>::from)?
        .with_inter_threads(1)
        .map_err(ort::Error::<()>::from)?
        .with_config_entry("session.intra_op.allow_spinning", "0")
        .map_err(ort::Error::<()>::from)?
        .with_config_entry("session.inter_op.allow_spinning", "0")
        .map_err(ort::Error::<()>::from)?
        .with_thread_manager(Threads)
        .map_err(ort::Error::<()>::from)
}

struct Threads;

impl ThreadManager for Threads {
    type Thread = JoinHandle<()>;

    fn create(&self, work: impl FnOnce() + Send + 'static) -> ort::Result<Self::Thread> {
        std::thread::Builder::new()
            .name("sailry-inference".into())
            .spawn(move || {
                background();
                work();
            })
            .map_err(ort::Error::wrap)
    }

    fn join(thread: Self::Thread) -> ort::Result<()> {
        thread
            .join()
            .map_err(|_| ort::Error::new("inference worker panicked"))
    }
}

#[cfg(target_os = "macos")]
pub fn background() {
    // SAFETY: changes only the calling worker's scheduling preference.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(target_os = "linux")]
pub fn background() {
    // SAFETY: Linux applies this nice value to the calling worker's native TID.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn background() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_owned_workers() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = Threads
            .create(move || sender.send(std::thread::current().id()).unwrap())
            .unwrap();
        Threads::join(worker).unwrap();
        assert_ne!(receiver.recv().unwrap(), std::thread::current().id());
    }

    #[test]
    fn bounds_cpu_workers() {
        assert!((1..=4).contains(&super::threads()));
    }
}
