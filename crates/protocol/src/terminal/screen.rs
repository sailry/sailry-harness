// Adapted from sailry-code 67ae9fa0, terminal_workspace.rs. See third_party_licenses/sailry-code-terminal.md.
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct TextStyle {
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    pub underline_color: Option<Rgb>,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub blink: bool,
    pub underline: UnderlineStyle,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Span {
    pub column: u16,
    pub columns: u16,
    pub text: String,
    pub style: TextStyle,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct Line {
    pub spans: Vec<Span>,
    pub wrapped: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Cursor {
    pub column: u16,
    pub row: u16,
    pub at_wide_tail: bool,
    pub blinking: bool,
    pub style: CursorStyle,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Screen {
    pub columns: u16,
    pub scrollback: Vec<Line>,
    pub rows: Vec<Line>,
    pub cursor: Option<Cursor>,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor_color: Rgb,
    pub bracketed_paste: bool,
    pub mouse_tracking: bool,
    pub alternate: bool,
    pub features: Features,
    pub graphics: Graphics,
}

impl Screen {
    pub fn validate(&self) -> Result<(), crate::Fault> {
        if !(1..=MAX_COLUMNS).contains(&self.columns)
            || self.rows.is_empty()
            || self.rows.len() > MAX_ROWS as usize
            || self.scrollback.len() > MAX_SCROLLBACK_ROWS
        {
            return Err(crate::Fault::invalid(
                "terminal screen dimensions are invalid",
            ));
        }
        if self.cursor.is_some_and(|cursor| {
            cursor.column >= self.columns || cursor.row as usize >= self.rows.len()
        }) {
            return Err(crate::Fault::invalid(
                "terminal cursor is outside the viewport",
            ));
        }
        let mut images = std::collections::BTreeMap::new();
        let mut allocation = 0_u64;
        for image in &self.graphics.images {
            allocation =
                allocation.saturating_add(u64::from(image.width) * u64::from(image.height) * 4);
            if image.width == 0
                || image.height == 0
                || allocation > MAX_GRAPHICS_BYTES as u64
                || image.png.len() > MAX_GRAPHICS_BYTES * 2
                || images.insert(image.id, image).is_some()
            {
                return Err(crate::Fault::invalid("terminal image resource is invalid"));
            }
        }
        for placement in &self.graphics.placements {
            let Some(image) = images.get(&placement.image) else {
                return Err(crate::Fault::invalid("terminal placement image is missing"));
            };
            let [x, y, width, height] = placement.source;
            if x.checked_add(width).is_none_or(|end| end > image.width)
                || y.checked_add(height).is_none_or(|end| end > image.height)
                || placement.cell.contains(&0)
                || placement
                    .tile
                    .is_some_and(|[column, row, columns, rows]| column >= columns || row >= rows)
            {
                return Err(crate::Fault::invalid(
                    "terminal placement geometry is invalid",
                ));
            }
        }
        for line in self.rows.iter().chain(&self.scrollback) {
            let mut end = 0;
            for span in &line.spans {
                if span.column < end || span.columns == 0 {
                    return Err(crate::Fault::invalid("terminal spans overlap"));
                }
                end = span
                    .column
                    .checked_add(span.columns)
                    .filter(|end| *end <= self.columns)
                    .ok_or_else(|| {
                        crate::Fault::invalid("terminal span is outside the viewport")
                    })?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Patch {
    pub dropped_scrollback_rows: u32,
    pub appended_scrollback: Vec<Line>,
    /// Complete current viewport rows; history alone is incremental.
    pub columns: u16,
    pub rows: Vec<Line>,
    pub cursor: Option<Cursor>,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor_color: Rgb,
    pub bracketed_paste: bool,
    pub mouse_tracking: bool,
    pub alternate: bool,
    pub features: Features,
    /// Omitted when image content and placement geometry are unchanged.
    pub graphics: Option<Graphics>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenUpdate {
    Replace { screen: Screen },
    Patch { patch: Patch },
}
