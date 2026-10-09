use crate::Error;

pub(super) fn create() -> Result<rayon::ThreadPool, Error> {
    Ok(rayon::ThreadPoolBuilder::new()
        .num_threads(threads())
        .thread_name(|index| format!("sailry-embedding-{index}"))
        .start_handler(|_| background())
        .build()?)
}

pub(super) fn threads() -> usize {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    (available / 2).clamp(1, 4)
}

#[cfg(target_os = "macos")]
pub fn background() {
    // SAFETY: changes only the calling inference worker's scheduling preference.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(target_os = "linux")]
pub fn background() {
    // SAFETY: Linux applies this nice value to the calling worker's native TID.
    unsafe {
        let tid = libc::gettid();
        libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, 10);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn background() {}
