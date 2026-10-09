//! Kit has no Office position viewer; verified source context remains selectable.
use super::*;
use gpui_kit::component::{dialog::DialogFooter, scroll::ScrollableElement};

pub(super) struct Viewer {
    binding: Binding,
    input: CitationRequest,
    page: CitationPage,
    text: Entity<crate::content::markdown::State>,
    history: Vec<u32>,
    pending: bool,
    closed: bool,
}

pub(super) fn open(
    binding: Binding,
    input: CitationRequest,
    page: CitationPage,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Viewer> {
    // Existing document previews require worktree transfers and cannot select typed Office positions or external Node roots.
    let viewer = cx.new(|cx| Viewer {
        binding,
        input,
        text: cx.new(|cx| crate::content::markdown::State::new(literal(&page.text), cx)),
        page,
        history: Vec::new(),
        pending: false,
        closed: false,
    });
    let result = viewer.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let next = viewer.clone();
        let previous = viewer.clone();
        let close = viewer.clone();
        let done = viewer.clone();
        let state = viewer.read(cx);
        dialog
            .form_title(tr("knowledge_source"))
            .w((window.viewport_size().width - px(48.)).min(px(800.)))
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| close.update(cx, |viewer, _| viewer.closed = true))
            .child(viewer.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("knowledge-citation-previous")
                            .debug_selector(|| "knowledge-citation-previous".into())
                            .label(tr("knowledge_previous"))
                            .disabled(state.pending || state.history.is_empty())
                            .on_click(move |_, window, cx| {
                                previous.update(cx, |viewer, cx| viewer.load(false, window, cx))
                            }),
                    )
                    .child(
                        Button::new("knowledge-citation-next")
                            .debug_selector(|| "knowledge-citation-next".into())
                            .label(tr("knowledge_next"))
                            .disabled(state.pending || state.page.next_offset.is_none())
                            .on_click(move |_, window, cx| {
                                next.update(cx, |viewer, cx| viewer.load(true, window, cx))
                            }),
                    )
                    .child(
                        Button::new("knowledge-citation-close")
                            .debug_selector(|| "knowledge-citation-close".into())
                            .label(tr("knowledge_close"))
                            .on_click(move |_, window, cx| {
                                done.update(cx, |viewer, _| viewer.closed = true);
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
    result
}

impl Viewer {
    fn load(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let offset = if forward {
            self.page.next_offset
        } else {
            self.history.last().copied()
        };
        let Some(offset) = offset else {
            return;
        };
        let mut input = self.input.clone();
        input.offset = offset;
        self.pending = true;
        let client = self.binding.client.clone();
        let requested = input.clone();
        let job = self.binding.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::ReadKnowledgeCitation(requested)))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |viewer, window, cx| {
                viewer.pending = false;
                if viewer.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::KnowledgeCitation(page))) if matches(&page, &input) => {
                        if forward {
                            viewer.history.push(viewer.page.offset);
                        } else {
                            viewer.history.pop();
                        }
                        viewer
                            .text
                            .update(cx, |text, cx| text.set_source(literal(&page.text), cx));
                        viewer.page = page;
                        viewer.input = input;
                    }
                    Ok(Err(error)) => failure(error_key(error.code), window, cx),
                    _ => failure("knowledge_citation_unavailable", window, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn literal(text: &str) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}\n{text}\n{fence}")
}

fn position(position: &Position) -> SharedString {
    match position {
        Position::Lines { start, end } => {
            rust_i18n::t!("knowledge_lines", start = start, end = end).into()
        }
        Position::Page { page } => rust_i18n::t!("knowledge_page", page = page).into(),
        Position::Cells { sheet, start, end } => {
            rust_i18n::t!("knowledge_cells", sheet = sheet, start = start, end = end).into()
        }
        Position::Slide { slide } => rust_i18n::t!("knowledge_slide", slide = slide).into(),
        Position::Section { name } => name.clone().into(),
        Position::Value { key, pointer } => format!("{key}{pointer}").into(),
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .min_w_0()
            .child(div().font_medium().child(self.page.title.clone()))
            .child(div().text_color(cx.theme().muted_foreground).child(format!(
                "{} · {} · {}",
                self.binding.host,
                self.page.citation.path,
                position(&self.page.citation.position)
            )))
            .child(
                div()
                    .id("knowledge-citation-context")
                    .debug_selector(|| "knowledge-citation-context".into())
                    .w_full()
                    .max_h((window.viewport_size().height - px(260.)).max(px(160.)))
                    .overflow_y_scrollbar()
                    .child(crate::content::markdown::View::new(&self.text)),
            )
            .when(self.pending, |view| view.child(spinner::Spinner::new()))
    }
}

#[cfg(test)]
mod tests;
