use gpui::{App, Context, Entity, IntoElement, RenderOnce, Window};

use super::{InputBaseState, InputExtras, InputModeKind, TextDecoration, TextareaMode};

mod links;
pub use links::TextareaLink;

// The pinned Kit exposes decorations only on EditorState. Composer highlighting
// needs the same renderer while retaining Textarea's auto-grow and Enter policy.
// These spans affect presentation only; the shared engine still owns all edits.
#[derive(Default)]
pub struct TextareaExtras {
    decorations: Vec<TextDecoration>,
    links: Vec<TextareaLink>,
    pressed: Option<(TextareaLink, gpui::Point<gpui::Pixels>)>,
    hovered: bool,
}

impl TextareaExtras {
    fn icon_color(&self, offset: usize, fallback: gpui::Hsla) -> gpui::Hsla {
        // The source marker is faded to make room for the SVG. Reuse its text
        // color, not that fade or the independent caret color.
        self.decorations
            .iter()
            .find(|span| span.range.contains(&offset))
            .and_then(|span| span.style.color)
            .unwrap_or(fallback)
    }
}

impl InputExtras for TextareaExtras {
    fn link_hovered(&self) -> bool {
        self.hovered
    }

    fn decoration_layers(&self) -> Vec<&[TextDecoration]> {
        vec![&self.decorations]
    }
}

impl InputModeKind for TextareaMode {
    const MULTI_LINE: bool = true;
    type Extras = TextareaExtras;

    fn reset_annotations(state: &mut InputBaseState<Self>) {
        state.extras.decorations.clear();
        state.extras.links.clear();
        state.extras.pressed = None;
        state.extras.hovered = false;
    }

    fn adjust_annotations(state: &mut InputBaseState<Self>, _: &std::ops::Range<usize>, _: usize) {
        Self::reset_annotations(state);
    }

    fn paint_annotations(state: &TextareaState, window: &mut Window, cx: &mut App) {
        use super::RopeExt;
        for link in &state.extras.links {
            if link.icon.is_some()
                && let Some(marker) = state.text.char_at(link.range.start)
                && let Some(bounds) =
                    state.range_to_bounds(&(link.range.start..link.range.start + marker.len_utf8()))
            {
                let color = state
                    .extras
                    .icon_color(link.range.start, state.editor_style.caret);
                link.paint_icon(bounds, color, window, cx);
            }
        }
    }

    fn on_click(
        state: &mut TextareaState,
        event: &gpui::MouseDownEvent,
        offset: usize,
        _: &mut Window,
        _: &mut Context<TextareaState>,
    ) -> bool {
        state.press_link(event, offset);
        false
    }

    fn on_mouse_up(
        state: &mut TextareaState,
        event: &gpui::MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        state.release_link(event, cx);
    }

    fn on_mouse_move(
        state: &mut TextareaState,
        offset: usize,
        event: &gpui::MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        state.hover_link(offset, event, cx);
    }

    fn clear_hover_state(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
        state.extras.pressed = None;
        if state.extras.hovered {
            state.extras.hovered = false;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_icon_uses_text_color_without_marker_fading() {
        let fallback = gpui::hsla(0., 0., 0., 1.);
        let color = gpui::hsla(0.6, 1., 0.5, 1.);
        let extras = TextareaExtras {
            decorations: vec![TextDecoration::new(
                2..9,
                gpui::HighlightStyle {
                    color: Some(color),
                    fade_out: Some(1.),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        assert_eq!(extras.icon_color(2, fallback), color);
        assert_eq!(extras.icon_color(5, fallback), color);
        assert_eq!(extras.icon_color(9, fallback), fallback);
    }
}

impl InputBaseState<TextareaMode> {
    /// Set sorted, non-overlapping presentation spans using UTF-8 byte offsets.
    /// Spans are cleared on text edits; callers recompute them for the new value.
    /// This does not alter selection, composition, text, or undo history.
    pub fn set_decorations(&mut self, spans: Vec<TextDecoration>, cx: &mut Context<Self>) {
        let spans = super::decorations::normalize(&self.text, spans);
        if self.extras.decorations != spans {
            self.extras.decorations = spans;
            cx.notify();
        }
    }

    /// Current presentation spans, normalized to valid UTF-8 boundaries.
    pub fn decorations(&self) -> &[TextDecoration] {
        &self.extras.decorations
    }
}

/// State for editing ordinary multi-line text.
///
/// This is the shared editing engine in its multi-line kind. Code-editor
/// facilities such as languages, diagnostics, folding, and LSP do not exist on
/// this type — those methods live on [`super::EditorState`].
pub type TextareaState = InputBaseState<TextareaMode>;

/// An unstyled ordinary multi-line text input.
#[derive(IntoElement)]
pub struct Textarea {
    presentation: super::InlineTokenPresentation,
    state: Entity<TextareaState>,
}

impl Textarea {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            presentation: Default::default(),
        }
    }
    /// The element each atomic token renders as; the input keeps editing and history.
    pub fn token<R: IntoElement>(
        mut self,
        render: impl Fn(&super::InlineTokenContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.presentation = self.presentation.token(render);
        self
    }
    /// Open a reference after a completed, unconsumed token click.
    pub fn on_token_click(
        mut self,
        listener: impl Fn(&super::InlineTokenClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.presentation = self.presentation.on_token_click(listener);
        self
    }
}

impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, _| {
            state.set_token_presentation(self.presentation)
        });
        self.state
    }
}
