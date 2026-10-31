//! Open and close motion shared by the collapsible shell panes.
use gpui_kit::{Pixels, px};
use std::ops::Range;

/// Openness within this distance of its target counts as settled, so float
/// residue from a reversed transition cannot leave a pane mid-motion.
const SETTLED: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Closed,
    Moving,
    Open,
}

impl Phase {
    pub(super) fn new(open: bool, openness: f32) -> Self {
        if open && openness >= 1. - SETTLED {
            Self::Open
        } else if !open && openness <= SETTLED {
            Self::Closed
        } else {
            Self::Moving
        }
    }

    /// Content stays mounted until the close motion has finished.
    pub(super) fn mounted(self) -> bool {
        self != Self::Closed
    }

    /// The rendered extent of a pane whose full width is `width`.
    pub(super) fn extent(self, width: Pixels, openness: f32) -> Pixels {
        match self {
            Self::Closed => px(0.),
            Self::Moving => width * openness.clamp(0., 1.),
            Self::Open => width,
        }
    }

    /// Resizing is offered only once the pane has settled open.
    pub(super) fn range(self, range: Range<Pixels>, extent: Pixels) -> Range<Pixels> {
        match self {
            Self::Open => range,
            _ => extent..extent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn settles_only_at_target() {
        assert_eq!(Phase::new(true, 1.), Phase::Open);
        assert_eq!(Phase::new(true, 0.9999), Phase::Open);
        assert_eq!(Phase::new(true, 0.), Phase::Moving);
        assert_eq!(Phase::new(false, 0.), Phase::Closed);
        assert_eq!(Phase::new(false, 0.00001), Phase::Closed);
        assert_eq!(Phase::new(false, 1.), Phase::Moving);
        assert_eq!(Phase::new(false, 0.5), Phase::Moving);
    }

    #[test]
    fn keeps_content_until_closed() {
        assert!(Phase::Open.mounted());
        assert!(Phase::Moving.mounted());
        assert!(!Phase::Closed.mounted());
    }

    #[test]
    fn scales_extent_while_moving() {
        let width = px(240.);
        assert_eq!(Phase::Open.extent(width, 0.2), width);
        assert_eq!(Phase::Closed.extent(width, 0.8), px(0.));
        assert_eq!(Phase::Moving.extent(width, 0.5), px(120.));
        assert_eq!(Phase::Moving.extent(width, 1.2), width);
    }

    #[test]
    fn fixes_range_until_open() {
        let range = px(200.)..px(480.);
        assert_eq!(Phase::Open.range(range.clone(), px(240.)), range);
        assert_eq!(
            Phase::Moving.range(range.clone(), px(96.)),
            px(96.)..px(96.)
        );
        assert_eq!(Phase::Closed.range(range, px(0.)), px(0.)..px(0.));
    }
}
