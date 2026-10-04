use super::*;

pub(super) struct ProjectedViewport {
    pub(super) rows: Vec<Line>,
    pub(super) cursor: Option<Cursor>,
    pub(super) foreground: Rgb,
    pub(super) background: Rgb,
    pub(super) cursor_color: Rgb,
    pub(super) bracketed_paste: bool,
    pub(super) mouse_tracking: bool,
    pub(super) colors: ProjectionColors,
}

pub(super) struct ProjectionColors {
    pub(super) foreground: RgbColor,
    pub(super) background: RgbColor,
    pub(super) cursor: RgbColor,
    palette: [RgbColor; 256],
}

pub(super) fn project_viewport<'alloc>(
    terminal: &Ghostty<'alloc, '_>,
    render_state: &mut RenderState<'alloc>,
    row_iterator: &mut RowIterator<'alloc>,
    cell_iterator: &mut CellIterator<'alloc>,
    expected_viewport: &Viewport,
) -> Result<ProjectedViewport, Fault> {
    let snapshot = check(render_state.update(terminal), "update Ghostty render state")?;
    let columns = check(snapshot.cols(), "read Ghostty render columns")?;
    let row_count = check(snapshot.rows(), "read Ghostty render rows")?;
    if columns != expected_viewport.columns || row_count != expected_viewport.rows {
        return Err(fail(format!(
            "Ghostty render viewport {columns}x{row_count} does not match {}x{}",
            expected_viewport.columns, expected_viewport.rows
        )));
    }

    let render_colors = check(snapshot.colors(), "read Ghostty render colors")?;
    let colors = ProjectionColors {
        foreground: render_colors.foreground,
        background: render_colors.background,
        cursor: render_colors.cursor.unwrap_or(render_colors.foreground),
        palette: render_colors.palette,
    };
    let cursor = if check(snapshot.cursor_visible(), "read Ghostty cursor visibility")? {
        if let Some(position) = check(snapshot.cursor_viewport(), "read Ghostty cursor position")? {
            let blinking = check(snapshot.cursor_blinking(), "read Ghostty cursor blinking")?;
            let visual_style = check(snapshot.cursor_visual_style(), "read Ghostty cursor style")?;
            Some(Cursor {
                column: position.x,
                row: position.y,
                at_wide_tail: position.at_wide_tail,
                blinking,
                style: match visual_style {
                    CursorVisualStyle::Bar => CursorStyle::Bar,
                    CursorVisualStyle::Block => CursorStyle::Block,
                    CursorVisualStyle::Underline => CursorStyle::Underline,
                    CursorVisualStyle::BlockHollow => CursorStyle::BlockHollow,
                    _ => CursorStyle::Block,
                },
            })
        } else {
            None
        }
    } else {
        None
    };

    let mut rows = Vec::with_capacity(usize::from(row_count));
    {
        let mut row_iteration = check(
            row_iterator.update(&snapshot),
            "iterate Ghostty render rows",
        )?;
        let mut row_index = 0_u32;
        while let Some(row) = row_iteration.next() {
            let wrapped = check(
                check(row.raw_row(), "read Ghostty render row")?.is_wrapped(),
                "read Ghostty row wrapping",
            )?;
            let mut spans = Vec::new();
            {
                let mut cells = check(cell_iterator.update(row), "iterate Ghostty render cells")?;
                let mut column = 0_u16;
                while let Some(cell) = cells.next() {
                    let raw = check(cell.raw_cell(), "read Ghostty render cell")?;
                    let wide = check(raw.wide(), "read Ghostty cell width")?;
                    if !matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                        let mut text = String::new();
                        if check(cell.graphemes_len(), "measure Ghostty cell grapheme")? > 0 {
                            check(cell.graphemes_utf8(&mut text), "read Ghostty cell grapheme")?;
                        }
                        let style = check(cell.style(), "read Ghostty cell style")?;
                        let hyperlink =
                            if check(raw.has_hyperlink(), "read Ghostty cell hyperlink state")? {
                                let reference = check(
                                    terminal.grid_ref(Point::Viewport(PointCoordinate {
                                        x: column,
                                        y: row_index,
                                    })),
                                    "resolve Ghostty viewport hyperlink",
                                )?;
                                project_hyperlink(&reference)?
                            } else {
                                None
                            };
                        if let Some(span) =
                            project_cell(column, wide, text, style, raw, hyperlink, &colors)?
                        {
                            push_span(&mut spans, span)?;
                        }
                    }
                    column = column
                        .checked_add(1)
                        .ok_or_else(|| fail("Ghostty render column overflowed".to_owned()))?;
                }
                if column != columns {
                    return Err(fail(format!(
                        "Ghostty render row has {column} cells, expected {columns}"
                    )));
                }
            }
            check(row.set_dirty(false), "clear Ghostty row dirty state")?;
            rows.push(Line { spans, wrapped });
            row_index += 1;
        }
    }
    check(
        snapshot.set_dirty(Dirty::Clean),
        "clear Ghostty render dirty state",
    )?;
    if rows.len() != usize::from(row_count) {
        return Err(fail(format!(
            "Ghostty rendered {} rows, expected {row_count}",
            rows.len()
        )));
    }
    let bracketed_paste = check(
        terminal.mode(Mode::BRACKETED_PASTE),
        "read Ghostty bracketed paste mode",
    )?;

    Ok(ProjectedViewport {
        rows,
        cursor,
        foreground: protocol_color(colors.foreground),
        background: protocol_color(colors.background),
        cursor_color: protocol_color(colors.cursor),
        bracketed_paste,
        mouse_tracking: check(terminal.is_mouse_tracking(), "read Ghostty mouse tracking")?,
        colors,
    })
}

pub(super) fn project_history(
    terminal: &Ghostty<'_, '_>,
    start: usize,
    end: usize,
    columns: u16,
    colors: &ProjectionColors,
) -> Result<Vec<Line>, Fault> {
    if start > end {
        return Err(fail("Ghostty scrollback range is invalid".to_owned()));
    }
    let mut lines = Vec::with_capacity(end - start);
    for row in start..end {
        let row = u32::try_from(row).map_err(|_| {
            fail("Ghostty scrollback row does not fit the terminal protocol".to_owned())
        })?;
        lines.push(project_history_line(terminal, row, columns, colors)?);
    }
    Ok(lines)
}

pub(super) fn project_history_line(
    terminal: &Ghostty<'_, '_>,
    row: u32,
    columns: u16,
    colors: &ProjectionColors,
) -> Result<Line, Fault> {
    let first = check(
        terminal.grid_ref(Point::History(PointCoordinate { x: 0, y: row })),
        "resolve Ghostty scrollback row",
    )?;
    let wrapped = check(
        check(first.row(), "read Ghostty scrollback row")?.is_wrapped(),
        "read Ghostty scrollback wrapping",
    )?;
    let mut spans = Vec::new();
    for column in 0..columns {
        let reference = check(
            terminal.grid_ref(Point::History(PointCoordinate { x: column, y: row })),
            "resolve Ghostty scrollback cell",
        )?;
        let raw = check(reference.cell(), "read Ghostty scrollback cell")?;
        let wide = check(raw.wide(), "read Ghostty scrollback cell width")?;
        if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
            continue;
        }
        let text = project_grapheme(&reference)?;
        let style = check(reference.style(), "read Ghostty scrollback style")?;
        let hyperlink = if check(
            raw.has_hyperlink(),
            "read Ghostty scrollback hyperlink state",
        )? {
            project_hyperlink(&reference)?
        } else {
            None
        };
        if let Some(span) = project_cell(column, wide, text, style, raw, hyperlink, colors)? {
            push_span(&mut spans, span)?;
        }
    }
    Ok(Line { spans, wrapped })
}

pub(super) fn project_cell(
    column: u16,
    wide: CellWide,
    mut text: String,
    style: Style,
    raw: Cell,
    hyperlink: Option<String>,
    colors: &ProjectionColors,
) -> Result<Option<Span>, Fault> {
    if text.starts_with('\u{10eeee}') {
        text = " ".into();
    }
    let mut projected_style = TextStyle {
        foreground: resolve_color(style.fg_color, &colors.palette, style.bold, true),
        background: resolve_color(style.bg_color, &colors.palette, false, false),
        underline_color: resolve_color(style.underline_color, &colors.palette, false, false),
        bold: style.bold,
        faint: style.faint,
        italic: style.italic,
        blink: style.blink,
        underline: match style.underline {
            Underline::None => UnderlineStyle::None,
            Underline::Single => UnderlineStyle::Single,
            Underline::Double => UnderlineStyle::Double,
            Underline::Curly => UnderlineStyle::Curly,
            Underline::Dotted => UnderlineStyle::Dotted,
            Underline::Dashed => UnderlineStyle::Dashed,
            _ => UnderlineStyle::None,
        },
        inverse: style.inverse,
        invisible: style.invisible,
        strikethrough: style.strikethrough,
        overline: style.overline,
    };
    projected_style.background = match check(raw.content_tag(), "read Ghostty cell content")? {
        CellContentTag::BgColorPalette => Some(protocol_color(
            colors.palette
                [usize::from(check(raw.bg_color_palette(), "read Ghostty cell palette color")?.0)],
        )),
        CellContentTag::BgColorRgb => Some(protocol_color(check(
            raw.bg_color_rgb(),
            "read Ghostty cell RGB color",
        )?)),
        CellContentTag::Codepoint | CellContentTag::CodepointGrapheme => projected_style.background,
    };

    if text.is_empty() {
        if projected_style == TextStyle::default() && hyperlink.is_none() {
            return Ok(None);
        }
        text.push(' ');
    }
    let columns = match wide {
        CellWide::Wide => 2,
        CellWide::Narrow => 1,
        CellWide::SpacerTail | CellWide::SpacerHead => return Ok(None),
    };
    Ok(Some(Span {
        column,
        columns,
        text,
        style: projected_style,
        hyperlink,
    }))
}

pub(super) fn push_span(spans: &mut Vec<Span>, span: Span) -> Result<(), Fault> {
    let mergeable = span.columns == 1 && span.text.is_ascii();
    if mergeable && let Some(previous) = spans.last_mut() {
        let previous_end = previous
            .column
            .checked_add(previous.columns)
            .ok_or_else(|| fail("Ghostty projected span column overflowed".to_owned()))?;
        let previous_mergeable =
            previous.text.is_ascii() && usize::from(previous.columns) == previous.text.len();
        if previous_end == span.column
            && previous_mergeable
            && previous.style == span.style
            && previous.hyperlink == span.hyperlink
        {
            previous.columns = previous
                .columns
                .checked_add(1)
                .ok_or_else(|| fail("Ghostty projected span width overflowed".to_owned()))?;
            previous.text.push_str(&span.text);
            return Ok(());
        }
    }
    spans.push(span);
    Ok(())
}

pub(super) fn project_grapheme(reference: &GridRef<'_>) -> Result<String, Fault> {
    let mut buffer = ['\0'; 8];
    let length = match reference.graphemes(&mut buffer) {
        Ok(length) => return Ok(buffer[..length].iter().collect()),
        Err(GhosttyError::OutOfSpace { required }) => required,
        Err(error) => return Err(ghostty_error("read Ghostty cell grapheme", error)),
    };
    let mut buffer = vec!['\0'; length];
    let length = check(
        reference.graphemes(&mut buffer),
        "read Ghostty cell grapheme",
    )?;
    Ok(buffer[..length].iter().collect())
}

pub(super) fn project_hyperlink(reference: &GridRef<'_>) -> Result<Option<String>, Fault> {
    let mut buffer = vec![0_u8; 256];
    let length = match reference.hyperlink_uri(&mut buffer) {
        Ok(length) => length,
        Err(GhosttyError::OutOfSpace { required }) => {
            buffer.resize(required, 0);
            check(
                reference.hyperlink_uri(&mut buffer),
                "read Ghostty cell hyperlink",
            )?
        }
        Err(error) => return Err(ghostty_error("read Ghostty cell hyperlink", error)),
    };
    buffer.truncate(length);
    Ok(String::from_utf8(buffer)
        .ok()
        .filter(|value| !value.is_empty()))
}

pub(super) fn resolve_color(
    color: StyleColor,
    palette: &[RgbColor; 256],
    bold: bool,
    brighten_bold: bool,
) -> Option<Rgb> {
    match color {
        StyleColor::None => None,
        StyleColor::Rgb(color) => Some(protocol_color(color)),
        StyleColor::Palette(PaletteIndex(index)) => {
            let mut index = usize::from(index);
            if brighten_bold && bold && index < 8 {
                index += 8;
            }
            Some(protocol_color(palette[index]))
        }
    }
}

pub(super) fn history_rows(terminal: &Ghostty<'_, '_>) -> Result<usize, Fault> {
    let scrollbar = check(terminal.scrollbar(), "read Ghostty scrollbar")?;
    let rows = scrollbar.total.checked_sub(scrollbar.len).ok_or_else(|| {
        fail(format!(
            "Ghostty scrollbar length {} exceeds total {}",
            scrollbar.len, scrollbar.total
        ))
    })?;
    usize::try_from(rows)
        .map_err(|_| fail("Ghostty scrollback size does not fit this platform".to_owned()))
}

pub(super) fn track_history_tail(
    terminal: &Ghostty<'_, '_>,
    history_rows: usize,
) -> Result<Option<TrackedGridRef>, Fault> {
    if history_rows == 0 {
        return Ok(None);
    }
    let row = u32::try_from(history_rows - 1).map_err(|_| {
        fail("Ghostty scrollback row does not fit the terminal protocol".to_owned())
    })?;
    check(
        terminal.track_grid_ref(Point::History(PointCoordinate { x: 0, y: row })),
        "track Ghostty scrollback tail",
    )
    .map(Some)
}

pub(super) fn configure_appearance(
    terminal: &mut Ghostty<'_, '_>,
    appearance: &Appearance,
) -> Result<(), Fault> {
    let foreground = ghostty_color(appearance.foreground);
    let background = ghostty_color(appearance.background);
    let mut base = Palette::default();
    for (index, color) in appearance.palette.into_iter().enumerate() {
        base.set(PaletteIndex(index as _), ghostty_color(color));
    }
    let palette = Palette::generate(
        Some(&base),
        None,
        background,
        foreground,
        appearance.color_scheme == ColorScheme::Light,
    );
    check(
        terminal.set_default_fg_color(Some(foreground)),
        "configure Ghostty foreground",
    )?;
    check(
        terminal.set_default_bg_color(Some(background)),
        "configure Ghostty background",
    )?;
    check(
        terminal.set_default_cursor_color(Some(foreground)),
        "configure Ghostty cursor color",
    )?;
    check(
        terminal.set_default_color_palette(Some(palette)),
        "configure Ghostty palette",
    )?;
    Ok(())
}

pub(super) fn resize(terminal: &mut Ghostty<'_, '_>, viewport: &Viewport) -> Result<(), Fault> {
    let cell_width = viewport.pixel_width / u32::from(viewport.columns);
    let cell_height = viewport.pixel_height / u32::from(viewport.rows);
    check(
        terminal.resize(viewport.columns, viewport.rows, cell_width, cell_height),
        "resize Ghostty terminal",
    )
}

pub(super) fn ghostty_scrollback_bytes(viewport: &Viewport, max_rows: usize) -> usize {
    if max_rows == 0 {
        return 0;
    }
    // The pinned Ghostty screen treats this option as a page-memory budget,
    // while Sailry exposes a bounded row projection.
    let retained_rows = max_rows.saturating_add(usize::from(viewport.rows));
    usize::from(viewport.columns)
        .saturating_mul(std::mem::size_of::<Cell>())
        .saturating_mul(retained_rows)
}

pub(super) fn empty_screen(viewport: &Viewport, appearance: &Appearance) -> Screen {
    Screen {
        columns: viewport.columns,
        scrollback: Vec::new(),
        rows: vec![Line::default(); usize::from(viewport.rows)],
        cursor: None,
        foreground: appearance.foreground,
        background: appearance.background,
        cursor_color: appearance.foreground,
        bracketed_paste: false,
        mouse_tracking: false,
        alternate: false,
        features: Default::default(),
        graphics: Default::default(),
    }
}

pub(super) fn ghostty_color(color: Rgb) -> RgbColor {
    RgbColor {
        r: color.red,
        g: color.green,
        b: color.blue,
    }
}

pub(super) fn protocol_color(color: RgbColor) -> Rgb {
    Rgb {
        red: color.r,
        green: color.g,
        blue: color.b,
    }
}
