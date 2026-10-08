//! A selected text excerpt and its window-coordinate action anchor.
use gpui_kit::{Bounds, Pixels};

#[derive(Clone, Debug)]
pub struct Excerpt {
    pub text: String,
    pub anchor: Bounds<Pixels>,
}
