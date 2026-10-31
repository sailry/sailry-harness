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
    Replace {},
    Wrap {},
    Bold {},
    Italic {},
    Strike {},
    InlineCode {},
    Link {},
    Block { block: Block },
    Insert { block: Insertion },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Block {
    Paragraph,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
    Bullet,
    Ordered,
    Task,
    Quote,
    Callout,
    Code,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Insertion {
    Table,
    Image,
    Rule,
    Code,
    Mermaid,
    Math,
    Chart,
    Html,
}

impl Action {
    pub fn edits(&self) -> bool {
        matches!(
            self,
            Self::Cut {}
                | Self::Paste {}
                | Self::Undo {}
                | Self::Redo {}
                | Self::Replace {}
                | Self::Bold {}
                | Self::Italic {}
                | Self::Strike {}
                | Self::InlineCode {}
                | Self::Link {}
                | Self::Block { .. }
                | Self::Insert { .. }
        )
    }

    /// The markdown editor command for formatting actions.
    fn command(&self) -> Option<crate::content::editor::Command> {
        use crate::content::editor::{Command, Insert, Kind, Style};
        Some(match self {
            Self::Copy {} => Command::Copy,
            Self::Cut {} => Command::Cut,
            Self::Paste {} => Command::Paste,
            Self::Undo {} => Command::Undo,
            Self::Redo {} => Command::Redo,
            Self::Find {} => Command::Find,
            Self::Replace {} => Command::Replace,
            Self::Bold {} => Command::Style(Style::Bold),
            Self::Italic {} => Command::Style(Style::Italic),
            Self::Strike {} => Command::Style(Style::Strike),
            Self::InlineCode {} => Command::Style(Style::Code),
            Self::Link {} => Command::Link,
            Self::Block { block } => Command::Kind(match block {
                Block::Paragraph => Kind::Paragraph,
                Block::Heading1 => Kind::Heading(1),
                Block::Heading2 => Kind::Heading(2),
                Block::Heading3 => Kind::Heading(3),
                Block::Heading4 => Kind::Heading(4),
                Block::Heading5 => Kind::Heading(5),
                Block::Heading6 => Kind::Heading(6),
                Block::Bullet => Kind::Bullet,
                Block::Ordered => Kind::Ordered,
                Block::Task => Kind::Task,
                Block::Quote => Kind::Quote,
                Block::Callout => Kind::Callout,
                Block::Code => Kind::Code,
            }),
            Self::Insert { block } => Command::Insert(match block {
                Insertion::Table => Insert::Table,
                Insertion::Image => Insert::Image,
                Insertion::Rule => Insert::Rule,
                Insertion::Code => Insert::Code,
                Insertion::Mermaid => Insert::Mermaid,
                Insertion::Math => Insert::Math,
                Insertion::Chart => Insert::Chart,
                Insertion::Html => Insert::Html,
            }),
            Self::Focus {}
            | Self::Source {}
            | Self::Document {}
            | Self::Reveal { .. }
            | Self::Wrap {} => return None,
        })
    }
}

/// The caret's formatting as the document toolbar reads it.
fn format(state: &crate::content::editor::State, cx: &App) -> Value {
    use crate::content::editor::Kind;
    let format = state.format(cx);
    let block = format.kind.map(|kind| match kind {
        Kind::Paragraph => "paragraph".to_owned(),
        Kind::Heading(level) => format!("heading{level}"),
        Kind::Bullet => "bullet".to_owned(),
        Kind::Ordered => "ordered".to_owned(),
        Kind::Task => "task".to_owned(),
        Kind::Quote => "quote".to_owned(),
        Kind::Callout => "callout".to_owned(),
        Kind::Code => "code".to_owned(),
    });
    json!({"bold":format.bold,"italic":format.italic,"strike":format.strike,"code":format.code,"link":format.link,"block":block})
}

pub(super) struct Document {
    pub path: String,
    pub input: Entity<EditorState>,
    pub markdown: Option<Entity<crate::content::editor::State>>,
    pub notebook: Option<super::notebook::Notebook>,
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
    /// Kit's code editor wraps by default and exposes no getter.
    pub wrap: bool,
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
        let preview = self
            .notebook
            .as_ref()
            .is_some_and(|notebook| notebook.preview);
        let can_cut_paste = self
            .markdown
            .as_ref()
            .is_none_or(|state| state.read(cx).can_cut_paste());
        let can_copy = self.markdown.as_ref().map_or_else(
            || !self.input.read(cx).selected_range().is_empty(),
            |state| state.read(cx).can_copy(cx),
        );
        json!({"id":id,"path":self.path,"lines":input.text().lines_len(),"position":{"line":position.line + 1,"column":position.character + 1},"language":input.language_name(),"revision":self.revision,"dirty":dirty,"saving":self.saving,"uncertain":self.uncertain,"truncated":self.truncated,
            "mode":if preview || mode == Some(crate::content::editor::Mode::Document) { "document" } else { "source" },
            "markdown":self.markdown.is_some(),"notebook":self.notebook.is_some(),"can_copy":preview || can_copy,"can_edit":self.revision.is_some() && !self.uncertain && !preview,"readonly":self.revision.is_none() || self.uncertain,
            "can_cut_paste":self.revision.is_some() && !self.uncertain && can_cut_paste && !preview,"can_save":self.revision.is_some() && (dirty || self.uncertain) && !self.saving,"error":self.error,
            "wrap":self.wrap,"format":self.markdown.as_ref().map(|state| format(state.read(cx), cx))})
    }

    pub fn action(&mut self, action: Action, window: &mut Window, cx: &mut App) {
        use crate::content::editor::Mode;
        if let Action::Wrap {} = action {
            self.wrap = !self.wrap;
            let wrap = self.wrap;
            self.input
                .update(cx, |input, cx| input.set_soft_wrap(wrap, window, cx));
            return;
        }
        if let Action::Reveal { line } = action {
            if let Some(notebook) = &mut self.notebook {
                notebook.preview = false;
            }
            self.input.update(cx, |input, cx| {
                input.set_cursor_position(
                    Position::new(line.saturating_sub(1).min(u32::MAX as usize) as u32, 0),
                    window,
                    cx,
                )
            });
            return;
        }
        if let Some(notebook) = &mut self.notebook {
            match &action {
                Action::Source {} | Action::Find {} => notebook.preview = false,
                Action::Document {} => {
                    notebook.preview = true;
                    notebook.focus(window, cx);
                    return;
                }
                Action::Focus {} if notebook.preview => {
                    notebook.focus(window, cx);
                    return;
                }
                Action::Copy {} if notebook.preview => {
                    notebook.copy(window, cx);
                    return;
                }
                action if notebook.preview && action.edits() => return,
                _ => {}
            }
        }
        if let Some(markdown) = &self.markdown {
            markdown.update(cx, |state, cx| match action {
                Action::Focus {} => state.focus_handle(cx).focus(window, cx),
                Action::Source {} => state.set_mode(Mode::Source, window, cx),
                Action::Document {} => state.set_mode(Mode::Document, window, cx),
                action => {
                    if let Some(command) = action.command() {
                        state.command(command, window, cx);
                    }
                }
            });
        } else {
            let focus = self.input.read(cx).focus_handle(cx);
            match action {
                Action::Copy {} => focus.dispatch_action(&Copy, window, cx),
                Action::Cut {} => focus.dispatch_action(&Cut, window, cx),
                Action::Paste {} => focus.dispatch_action(&Paste, window, cx),
                Action::Undo {} => focus.dispatch_action(&Undo, window, cx),
                Action::Redo {} => focus.dispatch_action(&Redo, window, cx),
                Action::Find {} | Action::Replace {} => {
                    let replace = matches!(action, Action::Replace {});
                    focus.focus(window, cx);
                    self.input
                        .update(cx, |input, cx| input.open_search(replace, cx));
                }
                // Code files have no Markdown formatting.
                Action::Bold {}
                | Action::Italic {}
                | Action::Strike {}
                | Action::InlineCode {}
                | Action::Link {}
                | Action::Block { .. }
                | Action::Insert { .. } => {}
                _ => focus.focus(window, cx),
            }
        }
    }
}
