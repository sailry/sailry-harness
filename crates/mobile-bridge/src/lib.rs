//! Thin no-page Mobile contract bridge; all projections and recovery stay in Client.
#[cfg(target_os = "android")]
mod android;
pub mod api;
mod frb_generated;
