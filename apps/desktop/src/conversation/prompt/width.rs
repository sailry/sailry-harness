//! Intrinsic sizing for sent reference prompts; Kit retains text and interaction ownership.
use super::Link;
use gpui_kit::component::input::{InlineTokenContext, InputToken};
use gpui_kit::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Measurement {
    key: Option<(Font, Pixels, Pixels, Pixels)>,
    tokens: BTreeMap<usize, Pixels>,
    natural: Option<Pixels>,
}

impl Measurement {
    pub(super) fn token(
        &mut self,
        context: &InlineTokenContext,
        source: &str,
        links: &[Link],
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let key = (
            style.font(),
            font_size,
            context.line_height(),
            context.available_width(),
        );
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.tokens.clear();
            self.natural = None;
        }
        // Measure the actual stateless Kit component without its row cap. The
        // renderer returns a separate, unmeasured InputToken with its normal cap:
        // returning this already-requested element would violate GPUI's phases.
        let mut token = InputToken::new(context)
            .max_w(Length::Auto)
            .into_any_element();
        let measured = token.layout_as_root(
            size(
                AvailableSpace::MaxContent,
                AvailableSpace::Definite(context.line_height()),
            ),
            window,
            cx,
        );
        self.tokens.insert(context.range().start, measured.width);
        if !links
            .iter()
            .all(|link| self.tokens.contains_key(&link.range.start))
        {
            return;
        }
        let mut longest = px(0.);
        let mut line = px(0.);
        let mut offset = 0;
        let mut text = |part: &str, line: &mut Pixels| {
            for (index, part) in part.split('\n').enumerate() {
                if index > 0 {
                    longest = longest.max(*line);
                    *line = px(0.);
                }
                *line += window
                    .text_system()
                    .shape_line(
                        part.to_owned().into(),
                        font_size,
                        &[style.to_run(part.len())],
                        None,
                    )
                    .width;
            }
        };
        for link in links {
            text(&source[offset..link.range.start], &mut line);
            line += self.tokens[&link.range.start];
            offset = link.range.end;
        }
        text(&source[offset..], &mut line);
        self.natural = Some(longest.max(line));
    }

    pub(super) fn width(&self, outer: Pixels) -> Option<Pixels> {
        let available = self.key.as_ref()?.3;
        // The public token context supplies the native row width after its
        // reserved caret margin. Compare this frame's outer prompt width to
        // measure the actual horizontal inset without copying Kit's constants.
        let inset = (outer - available).max(px(0.));
        Some(
            (self.natural? + inset)
                .ceil()
                .min(px(crate::conversation::layout::USER_MESSAGE_WIDTH)),
        )
    }
}
