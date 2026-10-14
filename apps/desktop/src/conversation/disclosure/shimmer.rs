//! Kit ShimmerText accepts plain text only. This activity-specific adapter keeps
//! mixed-font StyledText layout fixed and animates its public paint decorations.
//! GPUI owns native ellipsis, shaping, glyph positions and reduced motion.
use gpui_kit::{component::*, *};
use std::{collections::BTreeSet, time::Duration};

#[derive(IntoElement)]
pub(super) struct Summary {
    id: String,
    text: SharedString,
    detail_start: usize,
    detail: super::Detail,
}

impl Summary {
    pub fn new(id: String, label: SharedString, detail: &super::Detail) -> Self {
        let (text, detail_start) = if detail.text.is_empty() {
            (label.clone(), label.len())
        } else {
            (format!("{label} {}", detail.text).into(), label.len() + 1)
        };
        Self {
            id,
            text,
            detail_start,
            detail: detail.clone(),
        }
    }

    fn runs(&self, style: &TextStyle, mono: SharedString, secondary: Hsla) -> Vec<TextRun> {
        let mut runs = vec![style.to_run(self.detail_start)];
        if self.detail_start < self.text.len() {
            let mut detail = style.to_run(self.text.len() - self.detail_start);
            if self.detail.mono {
                detail.font.family = mono;
            }
            if self.detail.secondary && !self.detail.failed {
                detail.color = secondary;
            }
            runs.push(detail);
        }
        runs
    }
}

impl RenderOnce for Summary {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut style = window.text_style();
        let base = super::tone(&self.detail, cx);
        let hovered = style.color != base;
        style.color = base;
        let secondary = cx.theme().muted_foreground.opacity(0.65);
        let runs = self.runs(&style, cx.theme().mono_font_family.clone(), secondary);
        let text = StyledText::new(self.text).with_runs(runs);
        if cx.reduce_motion() || hovered {
            return text.into_any_element();
        }
        Glyphs {
            text,
            detail_start: self.detail_start,
            color: style.color,
            secondary: (self.detail.secondary && !self.detail.failed).then_some(secondary),
            phase: 0.,
        }
        .with_animation(
            self.id,
            Animation::new(Duration::from_secs(2)).repeat_synced(),
            |mut glyphs, phase| {
                glyphs.phase = phase;
                glyphs
            },
        )
        .into_any_element()
    }
}

struct Glyphs {
    text: StyledText,
    detail_start: usize,
    color: Hsla,
    secondary: Option<Hsla>,
    phase: f32,
}

impl Glyphs {
    fn color(base: Hsla, amount: f32) -> Hsla {
        // Preserve the semantic ink outside the band; the sweep only changes alpha.
        base.opacity(1. - 0.35 * amount * amount)
    }

    fn decorations(&self, line: &LineLayout, offset: usize, extent: Pixels) -> Vec<DecorationRun> {
        // Native shaping clusters keep combining marks and ligatures together.
        let boundaries: BTreeSet<_> = [0, line.len]
            .into_iter()
            .chain(
                line.runs
                    .iter()
                    .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)),
            )
            .collect();
        let boundaries: Vec<_> = boundaries.into_iter().collect();
        let band = (extent * 0.3).max(px(16.));
        let center = (extent + band * 2.) * self.phase - band;
        boundaries
            .windows(2)
            .map(|boundary| {
                let position =
                    (line.x_for_index(boundary[0]) + line.x_for_index(boundary[1])) * 0.5;
                let amount = (1. - ((position - center).abs() / band)).max(0.);
                let base = if offset + boundary[0] >= self.detail_start {
                    self.secondary.unwrap_or(self.color)
                } else {
                    self.color
                };
                DecorationRun {
                    len: (boundary[1] - boundary[0]) as u32,
                    color: Self::color(base, amount),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }
            })
            .collect()
    }
}

impl IntoElement for Glyphs {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Glyphs {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text.prepaint(id, inspector, bounds, state, window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.text.layout();
        let lines = layout.line_layouts();
        let extent = lines
            .iter()
            .map(|line| line.unwrapped_layout.width)
            .fold(px(0.), Pixels::max)
            .min(bounds.size.width)
            .max(px(1.));
        let mut origin = bounds.origin;
        let mut offset = 0;
        for line in lines {
            let line = &line.unwrapped_layout;
            let decorations = self.decorations(line, offset, extent);
            let _ = line.paint(
                origin,
                layout.line_height(),
                window.text_style().text_align,
                Some(bounds.size.width),
                &decorations,
                window,
                cx,
            );
            origin.y += layout.line_height();
            offset += line.len + 1;
        }
    }
}

#[cfg(test)]
mod tests;
