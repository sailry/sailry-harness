#[cfg(target_os = "android")]
mod android;
mod connection;
mod endpoint;
mod runtime;
mod streams;

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_init(callback: Option<runtime::Callback>) -> i32 {
    match callback {
        Some(callback) => runtime::init(callback),
        None => 1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_cancel(operation: usize) {
    runtime::state().cancel(operation);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_free(data: *mut u8, length: usize) {
    if !data.is_null() {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                data, length,
            )));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_debug_count(kind: usize) -> usize {
    runtime::state().debug_count(kind)
}
