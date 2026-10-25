// Adapted from Bezel 4a7505ab, crates/markdown (MIT). See third_party_licenses/bezel.md.
//! Inline decoration and display offsets; padding never enters copied or saved text.
use super::{
    doc::{Mark, Text},
    style::Theme,
};
use gpui_kit::{
    FontStyle, FontWeight, SharedString, StrikethroughStyle, TextRun, UnderlineStyle, font, px,
};
use std::ops::Range;

/// Inline content flattened for shaping: one string, its runs, and the ranges
/// that need painting underneath (link clicks, inline-code washes, chips).
pub(super) struct Flat {
    pub text: SharedString,
    pub runs: Vec<TextRun>,
    pub links: Vec<(Range<usize>, String)>,
    pub code: Vec<Range<usize>>,
    pub chips: Vec<Range<usize>>,
    pub offsets: Offsets,
    pub icons: Vec<Range<usize>>,
    pub references: Vec<(Range<usize>, SharedString)>,
    pub source_len: usize,
}

impl Flat {
    pub(super) fn hover_link(&mut self, index: Option<usize>) {
        let hovered =
            index.and_then(|index| self.links.iter().find(|(range, _)| range.contains(&index)));
        let mut start = 0;
        for run in &mut self.runs {
            let end = start + run.len;
            if hovered.is_none_or(|(range, _)| range.start >= end || range.end <= start) {
                run.underline = None;
            }
            start = end;
        }
    }
}

/// Convert inline marks to consecutive shaped runs.
pub(super) fn flatten(text: &Text, base_weight: FontWeight, theme: &Theme) -> Flat {
    flatten_with(text, base_weight, theme, &[])
}

pub(super) fn flatten_with(
    text: &Text,
    base_weight: FontWeight,
    theme: &Theme,
    annotations: &[super::render::Annotation],
) -> Flat {
    let mut cuts: Vec<usize> = text
        .marks
        .iter()
        .flat_map(|span| [span.range.start, span.range.end])
        .chain([0, text.text.len()])
        .chain(
            annotations
                .iter()
                .flat_map(|annotation| [annotation.range.start, annotation.range.end]),
        )
        .chain(annotations.iter().filter_map(|annotation| {
            annotation.icon.as_ref()?;
            Some(
                annotation.range.start
                    + text
                        .text
                        .get(annotation.range.start..)?
                        .chars()
                        .next()?
                        .len_utf8(),
            )
        }))
        .filter(|cut| *cut <= text.text.len())
        .collect();
    cuts.sort_unstable();
    cuts.dedup();

    let mut runs = Vec::new();
    let mut links: Vec<(Range<usize>, String)> = Vec::new();
    let mut code: Vec<Range<usize>> = Vec::new();
    let mut chips: Vec<Range<usize>> = Vec::new();

    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let covering = text
            .marks
            .iter()
            .filter(|span| span.range.start <= start && span.range.end >= end);

        let (mut bold, mut italic, mut mono, mut strike) = (false, false, false, false);
        let mut chip = false;
        let mut link = None;
        for span in covering {
            match &span.mark {
                Mark::Bold => bold = true,
                Mark::Italic => italic = true,
                Mark::Strike => strike = true,
                Mark::Code | Mark::Math => mono = true,
                Mark::Mention { url, .. } => {
                    chip = true;
                    link = Some(url.clone());
                }
                Mark::Link(url) | Mark::Image(url) => link = Some(url.clone()),
            }
        }

        if mono {
            match code.last_mut() {
                Some(range) if range.end == start => range.end = end,
                _ => code.push(start..end),
            }
        }
        if chip {
            match chips.last_mut() {
                Some(range) if range.end == start => range.end = end,
                _ => chips.push(start..end),
            }
        }
        if let Some(url) = &link {
            match links.last_mut() {
                Some((range, last)) if range.end == start && last == url => range.end = end,
                _ => links.push((start..end, url.clone())),
            }
        }

        let mut face = font(if mono {
            theme.font_mono.clone()
        } else {
            theme.font_body.clone()
        });
        face.weight = if bold && base_weight.0 < FontWeight::SEMIBOLD.0 {
            FontWeight::SEMIBOLD
        } else {
            base_weight
        };
        face.style = if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        let annotation = annotations
            .iter()
            .find(|annotation| annotation.range.start <= start && end <= annotation.range.end);
        let hidden = annotation
            .is_some_and(|annotation| annotation.icon.is_some() && start == annotation.range.start);
        let mut color = if link.is_some() || annotation.is_some() {
            theme.link
        } else if mono {
            theme.code_text
        } else if bold {
            theme.strong_color.unwrap_or(theme.text)
        } else {
            theme.text
        };
        if hidden {
            color.a = 0.;
        }
        runs.push(TextRun {
            len: end - start,
            font: face,
            color,
            background_color: None,
            underline: (link.is_some() && !chip).then_some(UnderlineStyle {
                color: Some(theme.link),
                thickness: px(1.0),
                wavy: false,
            }),
            strikethrough: strike.then_some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(theme.text_muted),
            }),
        });
    }

    Flat {
        text: text.text.clone().into(),
        runs,
        links,
        code,
        chips,
        offsets: Offsets::default(),
        icons: Vec::new(),
        references: annotations
            .iter()
            .filter_map(|annotation| {
                let icon = annotation.icon.clone()?;
                let width = text
                    .text
                    .get(annotation.range.start..)?
                    .chars()
                    .next()?
                    .len_utf8();
                Some((annotation.range.start..annotation.range.start + width, icon))
            })
            .collect(),
        source_len: text.text.len(),
    }
    .decorate()
}

#[derive(Clone, Default)]
pub(super) struct Offsets {
    gaps: Vec<(usize, Range<usize>)>,
}

impl Offsets {
    pub fn display(&self, source: usize) -> usize {
        source
            + self
                .gaps
                .iter()
                .take_while(|(at, _)| *at <= source)
                .map(|(_, range)| range.len())
                .sum::<usize>()
    }

    fn before(&self, source: usize) -> usize {
        source
            + self
                .gaps
                .iter()
                .take_while(|(at, _)| *at < source)
                .map(|(_, range)| range.len())
                .sum::<usize>()
    }

    pub fn source(&self, display: usize) -> usize {
        let mut skipped = 0;
        for (source, gap) in &self.gaps {
            if display < gap.start {
                break;
            }
            if display <= gap.end {
                return *source;
            }
            skipped += gap.len();
        }
        display.saturating_sub(skipped)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Inset {
    CodeEnd,
    Link,
    CodeStart,
}

impl Flat {
    fn decorate(mut self) -> Self {
        // Nonbreaking insets participate in shaping and wrapping, but the offset
        // map keeps selection, editing and clipboard content in source coordinates.
        let mut inserts = Vec::new();
        for (index, range) in self.code.iter().enumerate() {
            inserts.push((range.start, Inset::CodeStart, index));
            inserts.push((range.end, Inset::CodeEnd, index));
        }
        for (index, (range, _)) in self.links.iter().enumerate() {
            inserts.push((range.start, Inset::Link, index));
        }
        if inserts.is_empty() {
            return self;
        }
        inserts.sort_unstable();
        let mut code = self.code.clone();
        let mut icons = vec![0..0; self.links.len()];
        let mut output = String::new();
        let mut runs = Vec::new();
        let mut input = 0;
        let mut next = inserts.into_iter().peekable();
        for run in &self.runs {
            let end = input + run.len;
            while next.peek().is_some_and(|(at, _, _)| *at <= end) {
                let (at, kind, index) = next.next().unwrap();
                if at > input {
                    output.push_str(&self.text[input..at]);
                    runs.push(TextRun {
                        len: at - input,
                        ..run.clone()
                    });
                    input = at;
                }
                let start = output.len();
                // An em reserves the icon; word joiner keeps it with its label.
                let spacer = if kind == Inset::Link {
                    "\u{2001}\u{2060}"
                } else {
                    "\u{202f}"
                };
                output.push_str(spacer);
                let range = start..output.len();
                self.offsets.gaps.push((at, range.clone()));
                match kind {
                    Inset::CodeEnd => code[index].end = range.end,
                    Inset::Link => icons[index] = range,
                    Inset::CodeStart => code[index].start = range.start,
                }
                runs.push(TextRun {
                    len: spacer.len(),
                    color: gpui_kit::transparent_black(),
                    underline: None,
                    strikethrough: None,
                    ..run.clone()
                });
            }
            if input < end {
                output.push_str(&self.text[input..end]);
                runs.push(TextRun {
                    len: end - input,
                    ..run.clone()
                });
                input = end;
            }
        }
        for (index, (range, _)) in self.links.iter_mut().enumerate() {
            *range = icons[index].start..self.offsets.before(range.end);
        }
        for range in &mut self.chips {
            *range = self.offsets.before(range.start)..self.offsets.before(range.end);
        }
        for (range, _) in &mut self.references {
            *range = self.offsets.display(range.start)..self.offsets.before(range.end);
        }
        self.text = output.into();
        self.runs = runs;
        self.code = code;
        self.icons = icons;
        self
    }
}

#[cfg(test)]
mod tests;
