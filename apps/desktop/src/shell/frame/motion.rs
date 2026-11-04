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
    fn phases_and_extent() {
        for (scenario, check) in [
            (
                "settles_only_at_target",
                (|_scenario: &str| {
                    assert_eq!(Phase::new(true, 1.), Phase::Open, "settles only at target");
                    assert_eq!(
                        Phase::new(true, 0.9999),
                        Phase::Open,
                        "settles only at target"
                    );
                    assert_eq!(
                        Phase::new(true, 0.),
                        Phase::Moving,
                        "settles only at target"
                    );
                    assert_eq!(
                        Phase::new(false, 0.),
                        Phase::Closed,
                        "settles only at target"
                    );
                    assert_eq!(
                        Phase::new(false, 0.00001),
                        Phase::Closed,
                        "settles only at target"
                    );
                    assert_eq!(
                        Phase::new(false, 1.),
                        Phase::Moving,
                        "settles only at target"
                    );
                    assert_eq!(
                        Phase::new(false, 0.5),
                        Phase::Moving,
                        "settles only at target"
                    );
                }) as fn(&str),
            ),
            (
                "keeps_content_until_closed",
                (|_scenario: &str| {
                    assert!(Phase::Open.mounted(), "keeps content until closed");
                    assert!(Phase::Moving.mounted(), "keeps content until closed");
                    assert!(!Phase::Closed.mounted(), "keeps content until closed");
                }),
            ),
            (
                "scales_extent_while_moving",
                (|_scenario: &str| {
                    let width = px(240.);
                    assert_eq!(
                        Phase::Open.extent(width, 0.2),
                        width,
                        "scales extent while moving"
                    );
                    assert_eq!(
                        Phase::Closed.extent(width, 0.8),
                        px(0.),
                        "scales extent while moving"
                    );
                    assert_eq!(
                        Phase::Moving.extent(width, 0.5),
                        px(120.),
                        "scales extent while moving"
                    );
                    assert_eq!(
                        Phase::Moving.extent(width, 1.2),
                        width,
                        "scales extent while moving"
                    );
                }),
            ),
            (
                "fixes_range_until_open",
                (|_scenario: &str| {
                    let range = px(200.)..px(480.);
                    assert_eq!(
                        Phase::Open.range(range.clone(), px(240.)),
                        range,
                        "fixes range until open"
                    );
                    assert_eq!(
                        Phase::Moving.range(range.clone(), px(96.)),
                        px(96.)..px(96.),
                        "fixes range until open"
                    );
                    assert_eq!(
                        Phase::Closed.range(range, px(0.)),
                        px(0.)..px(0.),
                        "fixes range until open"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
