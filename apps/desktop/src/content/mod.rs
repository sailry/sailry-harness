//! Shared document presentation for conversations and resource views.
//!
//! Components consume content and emit editing or navigation events. Resource
//! ownership, requests and persistence stay with their existing feature modules.

pub(crate) mod diff;
pub(crate) mod editor;
pub(crate) mod images;
pub(crate) mod markdown;
pub(crate) mod syntax;

pub(crate) mod files;
