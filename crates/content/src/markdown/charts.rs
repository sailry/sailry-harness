//! Bounded chart data rendered by the framework's native chart components.
use sailry_protocol::chart::{Chart, Data};
mod view;
pub(super) use view::render;
#[cfg(test)]
mod tests;
