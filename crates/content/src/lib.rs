//! Shared document presentation on Kit's buffers, controls and active theme.
//! Resource loading, navigation, persistence and localization belong to the caller.
pub mod diff;
pub mod editor;
pub mod markdown;
pub mod media;
pub mod selection;
pub mod syntax;

mod labels;
pub use labels::Labels;

#[cfg(test)]
mod tests;
