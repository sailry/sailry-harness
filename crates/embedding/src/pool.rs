use crate::Error;

pub(super) fn create() -> Result<rayon::ThreadPool, Error> {
    Ok(rayon::ThreadPoolBuilder::new()
        .num_threads(threads())
        .thread_name(|index| format!("sailry-embedding-{index}"))
        .start_handler(|_| background())
        .build()?)
}

pub(super) fn threads() -> usize {
    sailry_model_assets::session::threads()
}

pub fn background() {
    sailry_model_assets::session::background()
}
