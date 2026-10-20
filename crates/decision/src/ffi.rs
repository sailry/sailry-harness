//! Offline bindings to llama.cpp b11756 (d80619397c2ba5cf1366c29f7d43699ca3a159b9).
//! See ffi/README.md for the exact header identity and generation settings.
#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unsafe_op_in_unsafe_fn,
    clippy::all
)]

include!("../ffi/generated/llama.rs");
pub(crate) mod backend {
    include!("../ffi/generated/backend.rs");
}
pub(crate) mod base {
    include!("../ffi/generated/base.rs");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pinned_value_layouts() {
        assert_eq!(std::mem::size_of::<llama_model_params>(), 80);
        assert_eq!(std::mem::align_of::<llama_model_params>(), 8);
        assert_eq!(std::mem::size_of::<llama_context_params>(), 168);
        assert_eq!(std::mem::align_of::<llama_context_params>(), 8);
        assert_eq!(std::mem::size_of::<llama_batch>(), 56);
        assert_eq!(std::mem::align_of::<llama_batch>(), 8);
    }
}
