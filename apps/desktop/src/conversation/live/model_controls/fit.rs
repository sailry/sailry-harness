//! Width fitting for the composer model chip.
//!
//! The chip gives up space in a fixed order: the channel is always a short
//! badge, the effort label collapses to an icon, and only then does the model
//! name shrink with GPUI's middle ellipsis. GPUI has no layout query for "would
//! this content fit", so each frame records the rendered widths and the
//! unconstrained text widths; the next render decides from them. The decision
//! uses widths that are the same with or without the collapse, so it is stable.
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Widths {
    /// Rendered chip width.
    chip: Pixels,
    /// Rendered and unconstrained model name widths.
    model: Pixels,
    natural: Pixels,
    /// Rendered effort control and unconstrained effort label widths.
    effort: Pixels,
    label: Pixels,
    /// Free toolbar space beside the chip, when the toolbar reports it.
    spare: Option<Pixels>,
}

impl Widths {
    /// Whether the effort label must collapse for the chip to fit within `cap`.
    pub(super) fn compact(&self, cap: Pixels) -> bool {
        if self.chip <= px(0.) {
            return false;
        }
        let room = self.spare.map_or(cap, |spare| cap.min(self.chip + spare));
        let squeezed = (self.natural - self.model).max(px(0.));
        let full = self.chip + squeezed + (self.label - self.effort);
        full > room + px(0.5)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Part {
    Chip,
    Model,
    Effort,
    Spare,
}

#[derive(Default)]
pub(super) struct Fit(Cell<Widths>);

impl Fit {
    pub(super) fn compact(&self, cap: Pixels) -> bool {
        self.0.get().compact(cap)
    }

    /// An overlay that records its parent's width as `part`. `text` is the
    /// unconstrained content, measured with the parent's text style.
    pub(super) fn probe(
        self: &Rc<Self>,
        part: Part,
        text: Option<SharedString>,
        view: EntityId,
    ) -> impl IntoElement {
        let fit = self.clone();
        canvas(
            move |bounds, window, _| {
                let natural = text.as_ref().map(|text| {
                    let style = window.text_style();
                    let size = style.font_size.to_pixels(window.rem_size());
                    window
                        .text_system()
                        .shape_line(text.clone(), size, &[style.to_run(text.len())], None)
                        .width
                });
                let mut widths = fit.0.get();
                let width = bounds.size.width;
                match part {
                    Part::Chip => widths.chip = width,
                    Part::Model => {
                        widths.model = width;
                        widths.natural = natural.unwrap_or(width);
                    }
                    Part::Effort => {
                        widths.effort = width;
                        widths.label = natural.unwrap_or(width);
                    }
                    Part::Spare => widths.spare = Some(width),
                }
                if differs(fit.0.get(), widths) {
                    fit.0.set(widths);
                    window.on_next_frame(move |_, cx| cx.notify(view));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

fn differs(old: Widths, new: Widths) -> bool {
    let apart = |a: Pixels, b: Pixels| (a - b).abs() > px(0.5);
    apart(old.chip, new.chip)
        || apart(old.model, new.model)
        || apart(old.natural, new.natural)
        || apart(old.effort, new.effort)
        || apart(old.label, new.label)
        || match (old.spare, new.spare) {
            (Some(old), Some(new)) => apart(old, new),
            (old, new) => old.is_some() != new.is_some(),
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn widths(chip: f32, model: f32, natural: f32, effort: f32, label: f32) -> Widths {
        Widths {
            chip: px(chip),
            model: px(model),
            natural: px(natural),
            effort: px(effort),
            label: px(label),
            spare: None,
        }
    }

    #[test]
    fn keeps_label_when_everything_fits() {
        assert!(!widths(200., 100., 100., 30., 30.).compact(px(260.)));
        assert!(!Widths::default().compact(px(260.)));
    }

    #[test]
    fn collapses_label_before_truncating_model() {
        // The model is squeezed by 20px while the label is shown.
        assert!(widths(260., 100., 120., 30., 30.).compact(px(260.)));
    }

    #[test]
    fn stays_collapsed_while_label_cannot_fit() {
        // Collapsed to a 16px icon; restoring the 30px label would overflow.
        assert!(widths(250., 120., 120., 16., 30.).compact(px(260.)));
        // Enough room to restore it.
        assert!(!widths(240., 120., 120., 16., 30.).compact(px(260.)));
    }

    #[test]
    fn uses_free_toolbar_space() {
        let narrow = Widths {
            spare: Some(px(10.)),
            ..widths(180., 100., 100., 16., 30.)
        };
        assert!(narrow.compact(px(220.)));
        let wide = Widths {
            spare: Some(px(80.)),
            ..narrow
        };
        assert!(!wide.compact(px(220.)));
    }
}
