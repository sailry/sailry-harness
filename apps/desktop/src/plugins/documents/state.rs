use super::*;
use gpui_kit::component::RopeExt;
use gpui_kit::component::input::{Copy, Cut, EditorState, Paste, Position, Redo, Undo};
use sailry_client::DocumentWrite;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Action {
    Focus {},
    Source {},
    Document {},
    Copy {},
    Cut {},
    Paste {},
    Undo {},
    Redo {},
    Find {},
    Reveal { line: usize },
}

impl Action {
    pub fn edits(&self) -> bool {
        matches!(
            self,
            Self::Cut {} | Self::Paste {} | Self::Undo {} | Self::Redo {}
        )
    }
}

pub(super) struct Document {
    pub path: String,
    pub input: Entity<EditorState>,
    pub markdown: Option<Entity<crate::content::editor::State>>,
    pub baseline: String,
    pub revision: Option<String>,
    pub truncated: bool,
    pub request: Option<DocumentWrite>,
    pub submitted_text: Option<String>,
    pub saving: bool,
    pub staging: Option<CancellationToken>,
    pub dismissed: bool,
    pub uncertain: bool,
    pub error: Option<Fault>,
    pub stop: CancellationToken,
    pub _observers: Vec<Subscription>,
}

impl Drop for Document {
    fn drop(&mut self) {
        if let Some(staging) = &self.staging {
            staging.cancel();
        }
        self.stop.cancel();
    }
}

impl Document {
    pub fn dirty(&self, cx: &App) -> bool {
        self.input.read(cx).value().as_ref() != self.baseline
    }
    pub fn unsaved(&self, cx: &App) -> bool {
        self.dirty(cx) || self.saving || self.uncertain
    }

    pub fn snapshot(&self, id: RequestId, cx: &App) -> Value {
        let dirty = self.dirty(cx);
        let input = self.input.read(cx);
        let position = self
            .markdown
            .as_ref()
            .and_then(|state| state.read(cx).source_cursor_position(cx))
            .unwrap_or_else(|| input.cursor_position());
        let mode = self.markdown.as_ref().map(|state| state.read(cx).mode());
        let can_cut_paste = self
            .markdown
            .as_ref()
            .is_none_or(|state| state.read(cx).can_cut_paste());
        let can_copy = self.markdown.as_ref().map_or_else(
            || !self.input.read(cx).selected_range().is_empty(),
            |state| state.read(cx).can_copy(cx),
        );
        json!({"id":id,"path":self.path,"lines":input.text().lines_len(),"position":{"line":position.line + 1,"column":position.character + 1},"language":input.language_name(),"revision":self.revision,"dirty":dirty,"saving":self.saving,"uncertain":self.uncertain,"truncated":self.truncated,
            "mode":if mode == Some(crate::content::editor::Mode::Document) { "document" } else { "source" },
            "markdown":self.markdown.is_some(),"can_copy":can_copy,"can_edit":self.revision.is_some() && !self.uncertain,"readonly":self.revision.is_none() || self.uncertain,
            "can_cut_paste":self.revision.is_some() && !self.uncertain && can_cut_paste,"can_save":self.revision.is_some() && (dirty || self.uncertain) && !self.saving,"error":self.error})
    }

    pub fn action(&mut self, action: Action, window: &mut Window, cx: &mut App) {
        use crate::content::editor::{Command, Mode};
        if let Action::Reveal { line } = action {
            self.input.update(cx, |input, cx| {
                input.set_cursor_position(
                    Position::new(line.saturating_sub(1).min(u32::MAX as usize) as u32, 0),
                    window,
                    cx,
                )
            });
            return;
        }
        if let Some(markdown) = &self.markdown {
            markdown.update(cx, |state, cx| match action {
                Action::Focus {} => state.focus_handle(cx).focus(window, cx),
                Action::Source {} => state.set_mode(Mode::Source, window, cx),
                Action::Document {} => state.set_mode(Mode::Document, window, cx),
                Action::Copy {} => state.command(Command::Copy, window, cx),
                Action::Cut {} => state.command(Command::Cut, window, cx),
                Action::Paste {} => state.command(Command::Paste, window, cx),
                Action::Undo {} => state.command(Command::Undo, window, cx),
                Action::Redo {} => state.command(Command::Redo, window, cx),
                Action::Find {} => state.command(Command::Find, window, cx),
                Action::Reveal { .. } => unreachable!(),
            });
        } else {
            self.input.update(cx, |input, cx| match action {
                Action::Copy {} => input.copy(&Copy, window, cx),
                Action::Cut {} => input.cut(&Cut, window, cx),
                Action::Paste {} => input.paste(&Paste, window, cx),
                Action::Undo {} => input.undo(&Undo, window, cx),
                Action::Redo {} => input.redo(&Redo, window, cx),
                Action::Find {} => input.open_search(false, cx),
                _ => input.focus(window, cx),
            });
        }
    }
}
