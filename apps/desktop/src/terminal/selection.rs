use super::{View, protocol};
use gpui_kit::{component::scroll::ScrollbarHandle, *};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Position {
    pub row: usize,
    pub column: u16,
}

#[derive(Default)]
pub(super) struct Selection {
    pub anchor: Option<Position>,
    pub end: Option<Position>,
    pub dragging: bool,
    pub rectangular: bool,
    pub(super) unit: Option<(Position, Position)>,
    pub(super) words: bool,
}

impl Selection {
    pub fn clear(&mut self) {
        self.anchor = None;
        self.end = None;
        self.dragging = false;
        self.rectangular = false;
        self.unit = None;
    }
    pub fn columns(&self, row: usize, columns: u16) -> Option<(u16, u16)> {
        let (anchor, endpoint) = (self.anchor?, self.end?);
        let (start, end) = (anchor.min(endpoint), anchor.max(endpoint));
        if row < start.row || row > end.row {
            return None;
        }
        if self.rectangular {
            let from = anchor.column.min(endpoint.column);
            let to = anchor.column.max(endpoint.column);
            return (from < to).then_some((from, to));
        }
        let from = if row == start.row { start.column } else { 0 };
        let to = if row == end.row { end.column } else { columns };
        (from < to).then_some((from, to))
    }
    pub fn text(&self, screen: &protocol::Screen) -> String {
        let mut output = String::new();
        let mut previous: Option<bool> = None;
        for (row, line) in screen.scrollback.iter().chain(&screen.rows).enumerate() {
            let Some((start, end)) = self.columns(row, screen.columns) else {
                continue;
            };
            if previous.is_some_and(|wrapped| !wrapped || self.rectangular) {
                output.push('\n');
            }
            let mut column = start;
            let mut text = String::new();
            for span in &line.spans {
                let span_end = span.column + span.columns;
                if span_end <= start || span.column >= end {
                    continue;
                }
                let from = start.max(span.column);
                let to = end.min(span_end);
                while column < from {
                    text.push(' ');
                    column += 1;
                }
                if span.text.is_ascii() && span.text.len() == span.columns as usize {
                    text.push_str(
                        &span.text[(from - span.column) as usize..(to - span.column) as usize],
                    );
                } else {
                    text.push_str(&span.text);
                }
                column = to;
            }
            output.push_str(text.trim_end_matches(' '));
            previous = Some(line.wrapped);
        }
        output
    }
}

impl View {
    pub(super) fn position(&self, point: Point<Pixels>) -> Position {
        let Some(snapshot) = &self.state.snapshot else {
            return Position::default();
        };
        let row = ((point.y - self.metrics.bounds.top() - self.scroll.offset().y)
            / self.metrics.cell.height)
            .floor()
            .max(0.) as usize;
        let column = ((point.x - self.metrics.bounds.left()) / self.metrics.cell.width)
            .round()
            .clamp(0., snapshot.screen.columns as f32) as u16;
        Position {
            row: row.min(snapshot.screen.scrollback.len() + snapshot.screen.rows.len() - 1),
            column,
        }
    }
    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        if (event.modifiers.platform || event.modifiers.control)
            && let Some(url) = self.link_at(event.position)
        {
            cx.open_url(&url);
            cx.stop_propagation();
            return;
        }
        if self.report_press(event, cx) {
            return;
        }
        let position = self.position(event.position);
        self.selection.rectangular = event.modifiers.alt;
        self.selection.words = event.click_count == 2;
        self.selection.unit = None;
        if !event.modifiers.shift || self.selection.anchor.is_none() {
            self.selection.anchor = Some(position);
        }
        self.selection.end = Some(position);
        self.selection.dragging = true;
        if event.click_count >= 2
            && !self.selection.rectangular
            && let Some(snapshot) = &self.state.snapshot
        {
            let (start, end) = extent(&snapshot.screen, position, self.selection.words);
            self.selection.anchor = Some(start);
            self.selection.end = Some(end);
            self.selection.unit = Some((start, end));
        }
        self.drag_point = event.position;
        self.autoscroll = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(30))
                    .await;
                let active = view
                    .update(cx, |view, cx| {
                        if !view.selection.dragging {
                            return false;
                        }
                        let bounds = view.metrics.bounds;
                        let distance = if view.drag_point.y < bounds.top() {
                            bounds.top() - view.drag_point.y
                        } else if view.drag_point.y > bounds.bottom() {
                            bounds.bottom() - view.drag_point.y
                        } else {
                            px(0.)
                        };
                        if distance != px(0.) {
                            view.scroll.move_by(distance.clamp(px(-60.), px(60.)));
                            view.extend_selection(view.position(view.drag_point));
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !active {
                    break;
                }
            }
        }));
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.report_release(event, cx);
        self.selection.dragging = false;
        self.autoscroll = None;
    }
    pub(super) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.platform || event.modifiers.control {
            cx.notify();
        }
        if self.report_motion(event, cx) {
            return;
        }
        self.drag_point = event.position;
        if self.selection.dragging {
            self.extend_selection(self.position(event.position));
            cx.notify();
        }
    }
    pub(super) fn wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.report_wheel(event, cx) {
            return;
        }
        self.scroll
            .move_by(event.delta.pixel_delta(self.metrics.cell.height).y);
        cx.stop_propagation();
        cx.notify();
    }
}

impl View {
    fn extend_selection(&mut self, position: Position) {
        if let Some((start, end)) = self.selection.unit
            && let Some(snapshot) = &self.state.snapshot
        {
            let (from, to) = extent(&snapshot.screen, position, self.selection.words);
            self.selection.anchor = Some(if position < start { end } else { start });
            self.selection.end = Some(if position < start { from } else { to });
        } else {
            self.selection.end = Some(position);
        }
    }
}

pub(super) fn extent(
    screen: &protocol::Screen,
    position: Position,
    words: bool,
) -> (Position, Position) {
    let lines: Vec<_> = screen.scrollback.iter().chain(&screen.rows).collect();
    let mut first = position.row.min(lines.len() - 1);
    let mut last = first;
    while first > 0 && lines[first - 1].wrapped {
        first -= 1;
    }
    while last + 1 < lines.len() && lines[last].wrapped {
        last += 1;
    }
    if !words {
        return (
            Position {
                row: first,
                column: 0,
            },
            Position {
                row: last,
                column: screen.columns,
            },
        );
    }
    let mut cells = Vec::new();
    for (row, line) in lines.iter().enumerate().take(last + 1).skip(first) {
        for (column, end, text) in super::text::cells(line, screen.columns) {
            let class = if text.chars().all(char::is_whitespace) {
                0
            } else if text
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                1
            } else {
                2
            };
            cells.push((
                Position { row, column },
                Position { row, column: end },
                class,
            ));
        }
    }
    let index = cells
        .iter()
        .position(|(start, end, _)| *start <= position && position < *end)
        .unwrap_or(cells.len().saturating_sub(1));
    let class = cells[index].2;
    let mut start = index;
    let mut end = index;
    while start > 0 && cells[start - 1].2 == class {
        start -= 1;
    }
    while end + 1 < cells.len() && cells[end + 1].2 == class {
        end += 1;
    }
    (cells[start].0, cells[end].1)
}
