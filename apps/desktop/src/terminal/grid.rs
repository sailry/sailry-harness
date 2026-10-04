use super::{View, protocol, viewport::Metrics};
use gpui_kit::{
    component::{ActiveTheme, scroll::ScrollbarHandle},
    *,
};
use std::collections::BTreeMap;

type Style = (
    Font,
    Pixels,
    protocol::Rgb,
    protocol::Rgb,
    Hsla,
    Hsla,
    Option<String>,
);

#[derive(Default)]
pub(super) struct Cache {
    style: Option<Style>,
    rows: BTreeMap<usize, Row>,
}

struct Row {
    source: protocol::Line,
    runs: Vec<Run>,
}
#[derive(Clone)]
struct Run {
    column: u16,
    columns: u16,
    line: ShapedLine,
    background: Option<Hsla>,
    underline: protocol::UnderlineStyle,
    decoration: Hsla,
    overline: bool,
    blink: bool,
}

pub(super) struct Drawing {
    rows: Vec<(usize, Vec<Run>)>,
    images: Vec<super::graphics::Placement>,
    metrics: Metrics,
    offset: Pixels,
    text_visible: bool,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    composition: Option<(Point<Pixels>, ShapedLine)>,
}

pub(super) fn color(color: protocol::Rgb) -> Hsla {
    rgb((u32::from(color.red) << 16) | (u32::from(color.green) << 8) | u32::from(color.blue)).into()
}

pub(super) fn prepare(
    view: &mut View,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut Context<View>,
) -> Drawing {
    let settings = crate::preferences::data(cx).terminal;
    let font_size = px(settings.font_size.into());
    let mut font = font(if settings.font_family.is_empty() {
        cx.theme().mono_font_family.clone()
    } else {
        settings.font_family.into()
    });
    font.features = FontFeatures::disable_ligatures();
    let run = TextRun {
        len: 1,
        font: font.clone(),
        color: cx.theme().foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let cell_width = window
        .text_system()
        .shape_line("M".into(), font_size, &[run], None)
        .width
        .max(px(1.));
    let cell_height = (font_size * 1.5).ceil();
    // The terminal grid is a product-specific drawing boundary; spacing is inherited
    // from its container, and the final partial row/column is not sent to the PTY.
    let metrics = Metrics {
        bounds,
        cell: size(cell_width, cell_height),
        columns: (bounds.size.width / cell_width)
            .floor()
            .clamp(1., protocol::MAX_COLUMNS as f32) as u16,
        rows: (bounds.size.height / cell_height)
            .floor()
            .clamp(1., protocol::MAX_ROWS as f32) as u16,
    };
    view.metrics = metrics;
    let viewport = protocol::Viewport {
        columns: metrics.columns,
        rows: metrics.rows,
        pixel_width: (cell_width * metrics.columns as f32).round().into(),
        pixel_height: (cell_height * metrics.rows as f32).round().into(),
    };
    // Every mounted controller must track its pane geometry, including unfocused splits.
    if view.controlling() && bounds.size.width > px(0.) && bounds.size.height > px(0.) {
        let revision = view.state.snapshot.as_ref().unwrap().info.revision;
        if view.resize.as_ref() != Some(&viewport)
            && view
                .connection
                .enqueue(sailry_protocol::Command::ResizeTerminal {
                    terminal: view.binding.id,
                    revision,
                    viewport: viewport.clone(),
                })
        {
            view.resize = Some(viewport);
        }
        let appearance = crate::theme::terminal(cx);
        if view.appearance.as_ref() != Some(&appearance)
            && view
                .connection
                .enqueue(sailry_protocol::Command::SetTerminalAppearance {
                    terminal: view.binding.id,
                    revision,
                    appearance: appearance.clone(),
                })
        {
            view.appearance = Some(appearance);
        }
    }
    let mut drawing = Drawing {
        rows: Vec::new(),
        images: Vec::new(),
        metrics,
        offset: px(0.),
        text_visible: view.text_visible,
        cursor: None,
        selections: Vec::new(),
        composition: None,
    };
    let Some(snapshot) = view.state.snapshot.clone() else {
        return drawing;
    };
    let screen = &snapshot.screen;
    let hovered_link = view.hovered_link(window);
    let style = (
        font.clone(),
        font_size,
        screen.foreground,
        screen.background,
        cx.theme().blue,
        cx.theme().blue_light,
        hovered_link.clone(),
    );
    if view.cache.style.as_ref() != Some(&style) {
        view.cache.rows.clear();
        view.cache.style = Some(style);
    }
    let count = screen.scrollback.len() + screen.rows.len();
    view.scroll.layout(
        bounds,
        size(
            bounds.size.width,
            (cell_height * count as f32).max(bounds.size.height),
        ),
    );
    let offset = view.scroll.offset().y;
    drawing.offset = offset;
    drawing.images = view.graphics.prepare(screen, metrics, offset, window);
    let start = (-offset / cell_height).floor().max(0.) as usize;
    let end = (start + metrics.rows as usize + 2).min(count);
    view.cache.rows.retain(|row, _| (start..end).contains(row));
    for row in start..end {
        let line = if row < screen.scrollback.len() {
            &screen.scrollback[row]
        } else {
            &screen.rows[row - screen.scrollback.len()]
        };
        if view
            .cache
            .rows
            .get(&row)
            .is_none_or(|cached| &cached.source != line)
        {
            let runs = line
                .spans
                .iter()
                .map(|span| {
                    let mut font = font.clone();
                    if span.style.bold {
                        font.weight = FontWeight::BOLD;
                    }
                    if span.style.italic {
                        font.style = FontStyle::Italic;
                    }
                    let (foreground, background) = if span.style.inverse {
                        (
                            span.style.background.unwrap_or(screen.background),
                            Some(span.style.foreground.unwrap_or(screen.foreground)),
                        )
                    } else {
                        (
                            span.style.foreground.unwrap_or(screen.foreground),
                            span.style.background,
                        )
                    };
                    let opacity = if span.style.invisible {
                        0.
                    } else if span.style.faint {
                        0.5
                    } else {
                        1.
                    };
                    let linked = span.hyperlink.is_some();
                    let foreground = if linked && span.hyperlink == hovered_link {
                        cx.theme().blue_light
                    } else if linked {
                        cx.theme().blue
                    } else {
                        color(foreground)
                    }
                    .opacity(opacity);
                    let underline = if linked {
                        protocol::UnderlineStyle::Single
                    } else {
                        span.style.underline
                    };
                    let decoration = if linked {
                        foreground
                    } else {
                        span.style
                            .underline_color
                            .map(|value| color(value).opacity(opacity))
                            .unwrap_or(foreground)
                    };
                    let run = TextRun {
                        len: span.text.len(),
                        font,
                        color: foreground,
                        background_color: None,
                        underline: matches!(
                            underline,
                            protocol::UnderlineStyle::Single | protocol::UnderlineStyle::Curly
                        )
                        .then(|| UnderlineStyle {
                            color: Some(decoration),
                            thickness: px(1.),
                            wavy: underline == protocol::UnderlineStyle::Curly,
                        }),
                        strikethrough: span.style.strikethrough.then_some(StrikethroughStyle {
                            color: None,
                            thickness: px(1.),
                        }),
                    };
                    Run {
                        column: span.column,
                        columns: span.columns,
                        background: background.map(color),
                        underline,
                        decoration,
                        overline: span.style.overline,
                        blink: span.style.blink,
                        line: window.text_system().shape_line(
                            span.text.clone().into(),
                            font_size,
                            &[run],
                            None,
                        ),
                    }
                })
                .collect();
            view.cache.rows.insert(
                row,
                Row {
                    source: line.clone(),
                    runs,
                },
            );
        }
        drawing.rows.push((row, view.cache.rows[&row].runs.clone()));
        if view.search.open {
            for &(start, end) in &view.search.matches {
                let from = if row == start.row { start.column } else { 0 };
                let to = if row == end.row {
                    end.column
                } else {
                    screen.columns
                };
                if row >= start.row && row <= end.row && from < to {
                    drawing.selections.push(fill(
                        Bounds::new(
                            point(
                                bounds.left() + cell_width * from as f32,
                                bounds.top() + offset + cell_height * row as f32,
                            ),
                            size(cell_width * (to - from) as f32, cell_height),
                        ),
                        cx.theme().selection.opacity(0.5),
                    ));
                }
            }
        }
        if let Some((from, to)) = view.selection.columns(row, screen.columns) {
            drawing.selections.push(fill(
                Bounds::new(
                    point(
                        bounds.left() + cell_width * from as f32,
                        bounds.top() + offset + cell_height * row as f32,
                    ),
                    size(cell_width * (to - from) as f32, cell_height),
                ),
                cx.theme().selection,
            ));
        }
    }
    if let Some(cursor) = screen.cursor {
        let row = screen.scrollback.len() + cursor.row as usize;
        if (start..end).contains(&row) {
            let column = cursor.column.saturating_sub(u16::from(cursor.at_wide_tail));
            let width = screen.rows[cursor.row as usize]
                .spans
                .iter()
                .find(|span| span.column == column && !span.text.is_ascii())
                .map_or(1, |span| span.columns);
            let cursor_width = cell_width * width as f32;
            let cursor_origin = point(
                bounds.left() + cell_width * column as f32,
                bounds.top() + offset + cell_height * row as f32,
            );
            if !view.composition.text.is_empty() {
                let text = &view.composition.text;
                let run = TextRun {
                    len: text.len(),
                    font,
                    color: color(screen.foreground),
                    background_color: Some(color(screen.background)),
                    underline: Some(UnderlineStyle {
                        color: None,
                        thickness: px(1.),
                        wavy: false,
                    }),
                    strikethrough: None,
                };
                drawing.composition = Some((
                    cursor_origin,
                    window
                        .text_system()
                        .shape_line(text.clone().into(), font_size, &[run], None),
                ));
            } else if !cursor.blinking || view.cursor_visible || !view.cursor_focused() {
                let cursor_bounds = Bounds::new(cursor_origin, size(cursor_width, cell_height));
                drawing.cursor = Some(
                    if !view.cursor_focused() || cursor.style == protocol::CursorStyle::BlockHollow
                    {
                        outline(
                            cursor_bounds,
                            color(screen.cursor_color),
                            BorderStyle::Solid,
                        )
                    } else {
                        match cursor.style {
                            protocol::CursorStyle::Bar => fill(
                                Bounds::new(cursor_origin, size(px(2.), cell_height)),
                                color(screen.cursor_color),
                            ),
                            protocol::CursorStyle::Underline => fill(
                                Bounds::new(
                                    point(cursor_origin.x, cursor_origin.y + cell_height - px(2.)),
                                    size(cursor_width, px(2.)),
                                ),
                                color(screen.cursor_color),
                            ),
                            _ => fill(cursor_bounds, color(screen.cursor_color).opacity(0.4)),
                        }
                    },
                );
            }
        }
    }
    drawing
}

pub(super) fn paint(
    view: &Entity<View>,
    bounds: Bounds<Pixels>,
    drawing: Drawing,
    window: &mut Window,
    cx: &mut App,
) {
    let focus = view.read(cx).focus.clone();
    window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        super::graphics::paint(&drawing.images, -1, window);
        for (row, runs) in &drawing.rows {
            let y = bounds.top() + drawing.offset + drawing.metrics.cell.height * *row as f32;
            for run in runs {
                if let Some(background) = run.background {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                bounds.left() + drawing.metrics.cell.width * run.column as f32,
                                y,
                            ),
                            size(
                                drawing.metrics.cell.width * run.columns as f32,
                                drawing.metrics.cell.height,
                            ),
                        ),
                        background,
                    ));
                }
            }
        }
        super::graphics::paint(&drawing.images, 0, window);
        for selection in drawing.selections {
            window.paint_quad(selection);
        }
        for (row, runs) in drawing.rows {
            let y = bounds.top() + drawing.offset + drawing.metrics.cell.height * row as f32;
            for run in runs {
                if run.blink && !drawing.text_visible {
                    continue;
                }
                let origin = point(
                    bounds.left() + drawing.metrics.cell.width * run.column as f32,
                    y,
                );
                let mask = Bounds::new(
                    origin,
                    size(
                        drawing.metrics.cell.width * run.columns as f32,
                        drawing.metrics.cell.height,
                    ),
                );
                decorations(&run, mask, window);
                window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
                    let _ = run.line.paint(
                        origin,
                        drawing.metrics.cell.height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                });
            }
        }
        super::graphics::paint(&drawing.images, 1, window);
        if let Some(cursor) = drawing.cursor {
            window.paint_quad(cursor);
        }
        if let Some((origin, line)) = drawing.composition {
            let _ = line.paint(
                origin,
                drawing.metrics.cell.height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    });
}

// GPUI's shaped text supports single/curly underlines. The terminal grid supplies
// the remaining ANSI decorations rather than flattening their distinct styles.
fn decorations(run: &Run, bounds: Bounds<Pixels>, window: &mut Window) {
    let bar = |window: &mut Window, x, y, width| {
        window.paint_quad(fill(
            Bounds::new(point(x, y), size(width, px(1.))),
            run.decoration,
        ))
    };
    if run.overline {
        bar(
            window,
            bounds.left(),
            bounds.top() + px(1.),
            bounds.size.width,
        );
    }
    let y = bounds.bottom() - px(2.);
    match run.underline {
        protocol::UnderlineStyle::Double => {
            bar(window, bounds.left(), y - px(2.), bounds.size.width);
            bar(window, bounds.left(), y, bounds.size.width);
        }
        protocol::UnderlineStyle::Dotted | protocol::UnderlineStyle::Dashed => {
            let width = if run.underline == protocol::UnderlineStyle::Dotted {
                px(1.)
            } else {
                px(4.)
            };
            let mut x = bounds.left();
            while x < bounds.right() {
                bar(window, x, y, width.min(bounds.right() - x));
                x += width + px(2.);
            }
        }
        _ => {}
    }
}
