//! The retained DataTable SDK previously exposed only scalar cells. Kit's native
//! TableDelegate supports composed cells and Sizable supports custom row heights.
//! These small presentation types keep exact scalar rows available to selection,
//! copying and dump consumers without replacing Kit's table or scroll behavior.
use super::*;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Cell {
    pub primary: Line,
    pub secondary: Option<Line>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Line {
    pub text: String,
    pub icon: Option<String>,
    pub tone: Option<Tone>,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Tone {
    Primary,
    Muted,
    #[serde(rename = "chart_2")]
    Chart2,
    #[serde(rename = "chart_3")]
    Chart3,
    #[serde(rename = "chart_4")]
    Chart4,
}

impl Tone {
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Primary => cx.theme().primary,
            Self::Muted => cx.theme().muted_foreground,
            Self::Chart2 => cx.theme().chart_2,
            Self::Chart3 => cx.theme().chart_3,
            Self::Chart4 => cx.theme().chart_4,
        }
    }
}

impl Line {
    fn render(
        &self,
        id: String,
        secondary: bool,
        alignment: Alignment,
        compact: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let text = self.text.clone();
        h_flex()
            .id(SharedString::from(id.clone()))
            .debug_selector(move || id.clone())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .text_sm()
            .h_4()
            .line_height(px(16.))
            .when(compact, |line| line.h(px(14.)).line_height(px(14.)))
            .items_center()
            .gap_1()
            .when(alignment == Alignment::End, |line| line.justify_end())
            .when(secondary, |line| {
                line.text_xs().text_color(cx.theme().muted_foreground)
            })
            .when_some(self.icon.clone(), |line, path| {
                line.child(Icon::default().path(path).size_3().text_color(
                    self.tone.map(|tone| tone.color(cx)).unwrap_or_else(|| {
                        if secondary {
                            cx.theme().muted_foreground
                        } else {
                            cx.theme().foreground
                        }
                    }),
                ))
            })
            .child(div().min_w_0().truncate().child(text))
    }
}

impl Cell {
    pub fn render(&self, id: &str, alignment: Alignment, compact: bool, cx: &App) -> Div {
        v_flex()
            .size_full()
            .min_w_0()
            .justify_center()
            .when(!compact && self.secondary.is_some(), |cell| cell.gap_1())
            .child(
                self.primary
                    .render(format!("{id}-primary"), false, alignment, compact, cx),
            )
            .when_some(self.secondary.as_ref(), |cell, line| {
                cell.child(line.render(format!("{id}-secondary"), true, alignment, compact, cx))
            })
    }
}
