//! Selective Bezel content port; GPUI, controls and syntax remain owned by Kit.
pub mod doc;
pub mod edit;
mod files;
mod images;
mod inline;
pub mod parse;
pub mod preview;
pub mod render;
mod scroll;
pub mod select;
pub mod selectable;
pub mod style;
pub mod typography;
mod view;

pub use doc::{Block, BlockKind, Doc, Mark, Part};
#[cfg(test)]
pub use parse::parse;
pub use parse::parse_ranges;
pub use render::{BlockLayouts, Editing, render_with};
pub use select::{Cursor, Selection};
pub use view::{State, View};

#[cfg(test)]
mod tests;
