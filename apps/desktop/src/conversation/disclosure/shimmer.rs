//! Kit ShimmerText accepts plain text only. This activity-specific adapter uses
//! public StyledText runs and one Animation to preserve mixed fonts in one scan.
//! GPUI remains responsible for layout, native ellipsis, glyphs and reduced motion.
use gpui_kit::{component::*, *};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(IntoElement)]
pub(super) struct Summary {
    id: String,
    text: SharedString,
    detail_start: usize,
    mono: bool,
    secondary: bool,
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
            mono: detail.mono,
            secondary: detail.secondary && !detail.failed,
        }
    }

    fn runs(&self, style: &TextStyle, mono: SharedString, secondary: Hsla) -> Vec<TextRun> {
        self.text
            .char_indices()
            .map(|(index, character)| {
                let mut run = style.to_run(character.len_utf8());
                if index >= self.detail_start {
                    if self.mono {
                        run.font.family = mono.clone();
                    }
                    if self.secondary {
                        run.color = secondary;
                    }
                }
                run
            })
            .collect()
    }
}

impl RenderOnce for Summary {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = window.text_style();
        let runs = self.runs(
            &style,
            cx.theme().mono_font_family.clone(),
            cx.theme().muted_foreground.opacity(0.65),
        );
        let text = StyledText::new(self.text.clone()).with_runs(runs.clone());
        if cx.reduce_motion() {
            return text.into_any_element();
        }
        let width = window.use_keyed_state(
            (ElementId::from(self.id.clone()), "width"),
            cx,
            |window, _| window.viewport_size().width,
        );
        let available = *width.read(cx);
        let shaped = window.text_system().shape_text(
            self.text.clone(),
            style.font_size.to_pixels(window.rem_size()),
            &runs,
            None,
            None,
        );
        let Ok(lines) = shaped else {
            return text.into_any_element();
        };
        let mut positions = Vec::with_capacity(runs.len());
        let mut offset = 0;
        let styles: BTreeMap<_, _> = self
            .text
            .char_indices()
            .map(|(index, _)| index)
            .zip(runs)
            .collect();
        let mut runs = Vec::new();
        let mut extent = px(0.);
        for (line_index, line) in lines.iter().enumerate() {
            let layout = &line.unwrapped_layout;
            extent = extent.max(layout.width);
            // Keep native shaping clusters together, including combining marks and ligatures.
            let boundaries: BTreeSet<_> = [0, line.text.len()]
                .into_iter()
                .chain(
                    layout
                        .runs
                        .iter()
                        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)),
                )
                .collect();
            let boundaries: Vec<_> = boundaries.into_iter().collect();
            for boundary in boundaries.windows(2) {
                let mut run = styles[&(offset + boundary[0])].clone();
                run.len = boundary[1] - boundary[0];
                runs.push(run);
                let left = layout.x_for_index(boundary[0]);
                let right = layout.x_for_index(boundary[1]);
                positions.push((left + right) * 0.5);
            }
            offset += line.text.len();
            if line_index + 1 < lines.len() {
                runs.push(styles[&offset].clone());
                positions.push(layout.width);
                offset += 1;
            }
        }
        let extent = extent.min(available).max(px(1.));
        let band = (extent * 0.3).max(px(16.));
        let highlight = cx.theme().foreground;
        let animated = text.with_animation(
            self.id,
            Animation::new(Duration::from_secs(2)).repeat_synced(),
            move |text, phase| {
                let center = (extent + band * 2.) * phase - band;
                let runs = runs
                    .iter()
                    .zip(&positions)
                    .map(|(run, position)| {
                        let mut run = run.clone();
                        let amount = (1. - ((*position - center).abs() / band)).max(0.);
                        run.color = run.color.mix(highlight, amount * amount);
                        run
                    })
                    .collect();
                text.with_runs(runs)
            },
        );
        div()
            .min_w_0()
            .max_w_full()
            .truncate()
            .on_prepaint(move |bounds, window, cx| {
                if (*width.read(cx) - bounds.size.width).abs() <= px(0.5) {
                    return;
                }
                let width = width.clone();
                window.defer(cx, move |window, cx| {
                    width.update(cx, |width, cx| {
                        *width = bounds.size.width;
                        cx.notify();
                    });
                    window.refresh();
                });
            })
            .child(animated)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
