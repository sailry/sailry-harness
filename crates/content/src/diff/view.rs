use gpui_kit::{
    component::{
        ActiveTheme,
        diff::{Diff, DiffHunkSeparator, DiffState},
        text::TextView,
    },
    *,
};
use std::sync::Arc;

/// The captured patch owns parsing, while Kit owns selection, focus, syntax,
/// virtualized rows and scroll state. Repeated renders never reset Kit state.
pub struct State {
    pub diff: Entity<DiffState>,
    source: Option<(Arc<str>, String, bool)>,
    generation: u64,
    parse: Option<Task<()>>,
    failed: bool,
    rows: usize,
    missing_newline_visible: bool,
}

impl State {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let diff = cx.new(|cx| DiffState::new([], cx).with_context_lines(None));
        cx.observe(&diff, |_, _, cx| cx.notify()).detach();
        Self {
            diff,
            source: None,
            generation: 0,
            parse: None,
            failed: false,
            rows: 0,
            missing_newline_visible: true,
        }
    }

    /// Hides only Kit's missing-newline metadata label; source and copy remain exact.
    pub fn missing_newline_visible(mut self, visible: bool) -> Self {
        self.missing_newline_visible = visible;
        self
    }

    pub fn set_patch(&mut self, text: &str, path: &str, added: bool, cx: &mut Context<Self>) {
        if self.source.as_ref().is_some_and(|(source, current, kind)| {
            source.as_ref() == text && current == path && *kind == added
        }) {
            return;
        }
        self.parse = None;
        self.generation += 1;
        let generation = self.generation;
        let source: Arc<str> = text.into();
        let path = path.to_owned();
        self.source = Some((source.clone(), path.clone(), added));
        self.failed = false;
        self.rows = text.lines().count();
        let job = cx
            .background_executor()
            .spawn(async move { super::files(&source, &path, added) });
        self.parse = Some(cx.spawn(async move |view, cx| {
            let parsed = job.await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.parse = None;
                match parsed {
                    Ok(files) => view.diff.update(cx, |state, cx| state.set_files(files, cx)),
                    Err(_) => {
                        view.failed = true;
                        view.diff.update(cx, |state, cx| state.set_files([], cx));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn summary(&self, cx: &App) -> (usize, usize, bool) {
        self.diff
            .read(cx)
            .files()
            .iter()
            .fold((0, 0, false), |(added, removed, binary), file| {
                (
                    added + file.additions(),
                    removed + file.deletions(),
                    binary || file.is_binary(),
                )
            })
    }

    pub fn row_count(&self) -> usize {
        self.rows
    }
}

impl Focusable for State {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.diff.read(cx).focus_handle(cx)
    }
}

impl Render for State {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.failed {
            // Invalid tool data remains inspectable as literal source; it never
            // becomes a second diff renderer or reconstructed source document.
            let source = self
                .source
                .as_ref()
                .map_or("", |(source, _, _)| source.as_ref());
            let fence = "`".repeat(
                source
                    .split(|value| value != '`')
                    .map(str::len)
                    .max()
                    .unwrap_or(0)
                    .max(2)
                    + 1,
            );
            return div()
                .size_full()
                .child(TextView::markdown(
                    format!("diff-source-{}", cx.entity_id()),
                    format!("{fence}text\n{source}\n{fence}"),
                ))
                .into_any_element();
        }
        Diff::new(&self.diff)
            .header_visible(false)
            .missing_newline_visible(self.missing_newline_visible)
            .hunk_separator(DiffHunkSeparator::Simple)
            .border_0()
            .bg(super::background(cx))
            .size_full()
            .into_any_element()
    }
}

#[derive(IntoElement)]
struct Inline {
    id: SharedString,
    text: Arc<str>,
    path: String,
    added: bool,
}
impl RenderOnce for Inline {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| State::new(cx));
        state.update(cx, |state, cx| {
            state.set_patch(&self.text, &self.path, self.added, cx)
        });
        let count = state.read(cx).row_count().clamp(1, 5);
        div()
            .id(self.id)
            .w_full()
            .h(window.rem_size()
                * (f32::from(cx.theme().mono_font_size) / 16.)
                * 1.6
                * count as f32)
            .child(state)
    }
}

pub fn rows(id: &str, text: &str, path: &str, added: bool) -> AnyElement {
    Inline {
        id: format!("diff-{id}").into(),
        text: text.into(),
        path: path.into(),
        added,
    }
    .into_any_element()
}

pub fn surface(id: &str, state: &Entity<State>) -> AnyElement {
    div()
        .id(SharedString::from(format!("diff-{id}")))
        .size_full()
        .child(state.clone())
        .into_any_element()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
