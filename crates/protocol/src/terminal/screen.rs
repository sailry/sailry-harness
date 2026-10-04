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
