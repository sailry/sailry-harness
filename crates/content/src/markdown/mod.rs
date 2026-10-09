//! Selective Bezel content port; GPUI, controls and syntax remain owned by Kit.
mod charts;
pub mod doc;
pub mod edit;
mod files;
mod images;
mod inline;
pub mod parse;
pub(crate) mod pointer;
pub mod preview;
pub mod render;
mod scroll;
pub mod select;
pub mod selectable;
pub mod style;
pub mod typography;
mod view;
pub mod visuals;

pub use doc::{Block, BlockKind, Doc, Mark, Part};
#[cfg(test)]
pub use parse::parse;
pub use parse::with_ranges;
pub use render::{BlockLayouts, Editing, with_selection};
pub use select::{Cursor, Selection};
pub use view::{State, View};

#[cfg(test)]
mod tests;
