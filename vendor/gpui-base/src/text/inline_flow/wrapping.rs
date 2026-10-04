//! The pinned Kit flow has no break-all policy or link-decoration measurement.
//! Extend its measured fragments while keeping every original source byte intact.
use super::*;
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn icon_advance(font_size: Pixels) -> Pixels {
    font_size * LINK_ICON_ADVANCE
}

pub(super) fn link_starts(links: &[(Range<usize>, LinkMark)]) -> Vec<(usize, LinkMark)> {
    let mut starts = Vec::new();
    let mut previous: Option<&(Range<usize>, LinkMark)> = None;
    for link in links {
        if previous.is_none_or(|prev| prev.0.end != link.0.start || prev.1 != link.1) {
            starts.push((link.0.start, link.1.clone()));
        }
        previous = Some(link);
    }
    starts
}

pub(super) fn break_all(
    items: &[MeasureItem],
    image_sizes: &[Option<Size<Pixels>>],
    objects: &[Option<MeasuredInlineObject>],
    text_style: &TextStyle,
    wrap_width: Option<Pixels>,
    icons: bool,
    code_inset: Pixels,
    window: &mut Window,
) -> Vec<Range<usize>> {
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let mut width = px(0.);
    for (item_ix, item) in items.iter().enumerate() {
        let item_start = offset;
        match item {
            MeasureItem::Text {
                text,
                highlights,
                links,
            } => {
                let codes = code_ranges(highlights);
                let starts = if icons {
                    link_starts(links)
                } else {
                    Vec::new()
                };
                for hard_line in text.split_inclusive('\n') {
                    let content = hard_line.trim_end_matches('\n');
                    let local_start = offset - item_start;
                    let highlights = slice_ranges(
                        highlights,
                        local_start,
                        local_start + content.len(),
                        |range, style| (range, style.clone()),
                    );
                    let mut clusters = Vec::new();
                    for (range, scale) in text_size_ranges(content.len(), &highlights) {
                        let segment = &content[range.clone()];
                        let runs = text_runs(
                            segment.len(),
                            text_style,
                            &slice_ranges(&highlights, range.start, range.end, |range, style| {
                                (range, style.clone())
                            }),
                        );
                        let line =
                            shape_line(segment.to_owned().into(), font_size * scale, &runs, window);
                        clusters.extend(graphemes(&line).into_iter().map(|(cluster, width)| {
                            (
                                (range.start + cluster.start)..(range.start + cluster.end),
                                width,
                            )
                        }));
                    }
                    for (range, mut advance) in clusters {
                        if starts
                            .iter()
                            .any(|(start, _)| *start == local_start + range.start)
                        {
                            // Keep the icon with the first grapheme of its label.
                            advance += icon_advance(font_size);
                        }
                        let code = codes.iter().find(|code| {
                            code.start <= local_start + range.start
                                && local_start + range.end <= code.end
                        });
                        let left = code.is_some_and(|code| code.start == local_start + range.start);
                        let right = code.is_some_and(|code| code.end == local_start + range.end);
                        if left {
                            advance += code_inset;
                        }
                        if right {
                            advance += code_inset;
                        }
                        let index = offset + range.start;
                        if wrap_width.is_some_and(|limit| width + advance > limit) && start < index
                        {
                            ranges.push(start..index);
                            start = index;
                            width = px(0.);
                        }
                        width += advance;
                    }
                    offset += hard_line.len();
                    if hard_line.ends_with('\n') {
                        ranges.push(start..offset - 1);
                        start = offset;
                        width = px(0.);
                    }
                }
            }
            MeasureItem::Image { .. } => {
                let advance = image_sizes[item_ix].expect("image size is measured").width;
                if wrap_width.is_some_and(|limit| width + advance > limit) && start < offset {
                    ranges.push(start..offset);
                    start = offset;
                    width = px(0.);
                }
                width += advance;
                offset += IMAGE_LEN;
            }
            MeasureItem::Object { .. } => {
                let advance = objects[item_ix]
                    .as_ref()
                    .expect("inline object is measured")
                    .metrics
                    .size
                    .width;
                if wrap_width.is_some_and(|limit| width + advance > limit) && start < offset {
                    ranges.push(start..offset);
                    start = offset;
                    width = px(0.);
                }
                width += advance;
                offset += IMAGE_LEN;
            }
        }
    }
    if start < offset || ranges.is_empty() {
        ranges.push(start..offset);
    }
    ranges
}

// Accumulate native glyph advances into Unicode graphemes, including combining
// marks and emoji sequences. Do not estimate widths from individual characters.
fn graphemes(line: &ShapedLine) -> Vec<(Range<usize>, Pixels)> {
    let mut clusters = line
        .text
        .grapheme_indices(true)
        .map(|(start, text)| (start..start + text.len(), px(0.)))
        .collect::<Vec<_>>();
    let mut glyphs = line.runs.iter().flat_map(|run| &run.glyphs).peekable();
    while let Some(glyph) = glyphs.next() {
        let next_x = glyphs.peek().map_or(line.width, |next| next.position.x);
        let index = clusters.partition_point(|(range, _)| range.start <= glyph.index);
        if let Some((_, width)) = index
            .checked_sub(1)
            .and_then(|index| clusters.get_mut(index))
        {
            *width += next_x - glyph.position.x;
        }
    }
    clusters
}
