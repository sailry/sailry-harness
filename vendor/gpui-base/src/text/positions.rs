//! Batch lookup for Inline's per-character geometry using GPUI's shaped output.
use gpui::{Pixels, Point, TextLayout, WrappedLineLayout, point};

pub(super) struct Positions {
    lines: Vec<Line>,
}

struct Line {
    start: usize,
    end: usize,
    origin: Point<Pixels>,
    width: Pixels,
    glyphs: Vec<(usize, Pixels)>,
    rows: Vec<(usize, Point<Pixels>)>,
}

impl Positions {
    pub(super) fn new(layout: &TextLayout) -> Self {
        let mut origin = layout.bounds().origin;
        let height = layout.line_height();
        let mut start = 0;
        let lines = layout
            .line_layouts()
            .iter()
            .map(|layout| {
                let line = Line::new(layout, start, origin, height);
                origin.y += layout.size(height).height;
                start += layout.len() + 1;
                line
            })
            .collect();
        Self { lines }
    }

    pub(super) fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        self.lines
            .get(self.lines.partition_point(|line| line.end < index))?
            .position_for_index(index)
    }
}

impl Line {
    fn new(
        layout: &WrappedLineLayout,
        start: usize,
        origin: Point<Pixels>,
        height: Pixels,
    ) -> Self {
        let mut line = Self {
            start,
            end: start + layout.len(),
            origin,
            width: layout.unwrapped_layout.width,
            glyphs: Vec::new(),
            rows: Vec::new(),
        };
        // GPUI returns the first glyph whose byte index is >= the query. Only
        // successive record highs can be that first match. Do not sort glyphs:
        // shaping can repeat or reorder indices across combining/RTL runs.
        for glyph in layout
            .unwrapped_layout
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
        {
            if line
                .glyphs
                .last()
                .is_none_or(|(index, _)| *index < glyph.index)
            {
                line.glyphs.push((glyph.index, glyph.position.x));
            }
        }
        let ends = layout.wrap_boundaries.iter().map(|boundary| {
            layout.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
        });
        let mut row_start = 0;
        for (row, end) in ends.chain([layout.len()]).enumerate() {
            if line.rows.last().is_none_or(|(index, _)| *index < end) {
                line.rows
                    .push((end, point(line.x(row_start), row as f32 * height)));
            }
            row_start = end;
        }
        line
    }

    fn x(&self, index: usize) -> Pixels {
        self.glyphs
            .get(self.glyphs.partition_point(|(end, _)| *end < index))
            .map_or(self.width, |(_, x)| *x)
    }

    fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        if index < self.start || index > self.end {
            return None;
        }
        let index = index - self.start;
        let (_, offset) = self
            .rows
            .get(self.rows.partition_point(|(end, _)| *end < index))?;
        Some(self.origin + point(self.x(index) - offset.x, offset.y))
    }
}

#[cfg(test)]
mod tests;
