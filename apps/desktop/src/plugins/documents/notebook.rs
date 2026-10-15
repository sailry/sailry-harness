//! Notebook previews retain cell surfaces without starting a kernel.
use super::*;
use base64::Engine as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Copy, Editor, EditorState},
    scroll::ScrollableElement,
    v_flex,
};
use sailry_protocol::{Command, Output, notebook};
use std::sync::Arc;

pub(super) async fn load(
    client: Arc<sailry_client::Client>,
    worktree: WorktreeId,
    path: String,
    context: Option<plugin::Context>,
    stop: CancellationToken,
) -> Result<(sailry_protocol::FileContent, Option<notebook::Page>), Fault> {
    let content = match &context {
        Some(context) => {
            client
                .read_document_scoped(context, &path, stop.clone())
                .await?
        }
        None => client.read_document(worktree, &path, stop.clone()).await?,
    };
    if !path
        .rsplit('.')
        .next()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ipynb"))
    {
        return Ok((content, None));
    }
    let mut request = client.prepare(Command::ReadNotebook {
        worktree,
        options: notebook::Read {
            path: path.clone(),
            offset: 0,
            limit: 20,
        },
    });
    if let Some(context) = context {
        request = request.with_plugin(context);
    }
    let result = tokio::select! {
        _ = stop.cancelled() => Err(unavailable()),
        result = client.execute(request) => result,
    }?;
    let Output::NotebookPage(page) = result else {
        return Err(Fault::invalid("invalid notebook page response"));
    };
    if page.path != path
        || content
            .revision
            .as_ref()
            .is_some_and(|revision| *revision != page.revision)
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "notebook changed while opening",
        ));
    }
    Ok((content, Some(page)))
}

pub(in crate::plugins) struct Notebook {
    pub(in crate::plugins) preview: bool,
    pub(in crate::plugins) revision: String,
    pub(in crate::plugins) next: Option<usize>,
    pub(in crate::plugins) loading: bool,
    cells: Vec<Cell>,
    language: String,
}

struct Cell {
    index: usize,
    kind: String,
    source: Entity<EditorState>,
    markdown: Option<Entity<crate::content::markdown::State>>,
    outputs: Vec<Entity<EditorState>>,
    images: Vec<Arc<RenderImage>>,
    truncated: bool,
}

impl Notebook {
    pub(in crate::plugins) fn new(page: notebook::Page, window: &mut Window, cx: &mut App) -> Self {
        let language = page.metadata["kernelspec"]["language"]
            .as_str()
            .unwrap_or("text")
            .to_owned();
        let mut state = Self {
            preview: true,
            revision: page.revision.clone(),
            next: None,
            loading: false,
            cells: Vec::new(),
            language,
        };
        state.append(page, window, cx);
        state
    }

    pub(in crate::plugins) fn append(
        &mut self,
        page: notebook::Page,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.next = page.next_offset;
        for cell in page.cells {
            let kind = cell.value["cell_type"].as_str().unwrap_or("raw").to_owned();
            let source_text = text(&cell.value["source"]);
            let source = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language(if kind == "code" {
                        &self.language
                    } else {
                        "text"
                    })
                    .line_number(true)
                    .default_value(source_text.clone())
            });
            let markdown = (kind == "markdown")
                .then(|| cx.new(|cx| crate::content::markdown::State::new(source_text, cx)));
            let outputs = cell.value["outputs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|output| {
                    let value = if output["output_type"] == "stream" {
                        text(&output["text"])
                    } else if output["output_type"] == "error" {
                        output["traceback"]
                            .as_array()
                            .map(|lines| lines.iter().map(text).collect::<Vec<_>>().join("\n"))
                            .unwrap_or_else(|| {
                                format!(
                                    "{}: {}",
                                    output["ename"].as_str().unwrap_or_default(),
                                    output["evalue"].as_str().unwrap_or_default()
                                )
                            })
                    } else {
                        text(&output["data"]["text/plain"])
                    };
                    (!value.is_empty())
                        .then(|| cx.new(|cx| EditorState::new(window, cx).default_value(value)))
                })
                .collect();
            let images = cell.value["outputs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|output| {
                    [
                        ("image/png", image::ImageFormat::Png),
                        ("image/jpeg", image::ImageFormat::Jpeg),
                    ]
                    .into_iter()
                    .find_map(|(name, format)| {
                        let value = text(&output["data"][name]);
                        if value.len() > notebook::MAX_PAGE_BYTES {
                            return None;
                        }
                        let value: String = value
                            .chars()
                            .filter(|character| !character.is_ascii_whitespace())
                            .collect();
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(value)
                            .ok()?;
                        crate::content::images::decode::decode(&bytes, format, 1024).ok()
                    })
                })
                .collect();
            self.cells.push(Cell {
                index: cell.index,
                kind,
                source,
                markdown,
                outputs,
                images,
                truncated: cell.truncated,
            });
        }
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(cell) = self.cells.first() {
            if let Some(markdown) = &cell.markdown {
                markdown.read(cx).focus_handle(cx).focus(window, cx);
            } else {
                cell.source
                    .update(cx, |source, cx| source.focus(window, cx));
            }
        } else {
            window.blur(cx);
        }
    }

    pub(super) fn copy(&self, window: &mut Window, cx: &mut App) {
        for cell in &self.cells {
            if let Some(markdown) = &cell.markdown
                && markdown.read(cx).focus_handle(cx).is_focused(window)
            {
                let text = markdown.read(cx).selected_text();
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                return;
            }
            for source in std::iter::once(&cell.source).chain(&cell.outputs) {
                if source.read(cx).focus_handle(cx).is_focused(window) {
                    let focus = source.read(cx).focus_handle(cx);
                    focus.dispatch_action(&Copy, window, cx);
                    return;
                }
            }
        }
    }

    pub(super) fn draft(
        &mut self,
        source: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(), Fault> {
        let value: Value = serde_json::from_str(source)
            .map_err(|_| Fault::invalid("notebook draft is not valid JSON"))?;
        if value["nbformat"] != 4 || !value["metadata"].is_object() {
            return Err(Fault::invalid("notebook draft must use nbformat 4"));
        }
        let cells = value["cells"]
            .as_array()
            .ok_or_else(|| Fault::invalid("notebook draft cells must be an array"))?;
        if cells.iter().any(|cell| {
            !matches!(
                cell["cell_type"].as_str(),
                Some("code" | "markdown" | "raw")
            ) || !(cell["source"].is_string()
                || cell["source"]
                    .as_array()
                    .is_some_and(|lines| lines.iter().all(Value::is_string)))
        }) {
            return Err(Fault::invalid("invalid notebook draft cell"));
        }
        let page = notebook::Page {
            path: String::new(),
            revision: self.revision.clone(),
            nbformat: 4,
            nbformat_minor: 0,
            metadata: value["metadata"].clone(),
            metadata_truncated: false,
            total: cells.len(),
            cells: cells
                .iter()
                .enumerate()
                .map(|(index, value)| notebook::Cell {
                    index,
                    value: value.clone(),
                    truncated: false,
                })
                .collect(),
            next_offset: None,
        };
        *self = Self::new(page, window, cx);
        Ok(())
    }

    pub(in crate::plugins) fn view(&self, id: impl Into<ElementId>, cx: &App) -> AnyElement {
        let mut list = v_flex()
            .id(id)
            .gap_3()
            .p_3()
            .w_full()
            .min_h_0()
            .debug_selector(|| "notebook-preview".into());
        if self.cells.is_empty() {
            list = list.child(crate::tr("notebook_empty"));
        }
        for cell in &self.cells {
            let label = rust_i18n::t!("notebook_cell", number = cell.index + 1).to_string();
            let source = cell
                .markdown
                .as_ref()
                .map(|state| crate::content::markdown::View::new(state).into_any_element())
                .unwrap_or_else(|| editor(&cell.source, &label, cx));
            let mut content = v_flex()
                .id(("notebook-cell", cell.index))
                .gap_2()
                .p_3()
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().font_medium().child(label.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(cell.kind.clone()),
                        ),
                )
                .child(source);
            for output in &cell.outputs {
                content = content.child(editor(output, &label, cx));
            }
            for image in &cell.images {
                content = content.child(
                    img(image.clone())
                        .max_w_full()
                        .max_h(px(512.))
                        .object_fit(ObjectFit::Contain),
                );
            }
            if cell.truncated {
                content = content.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(crate::tr("files_content_partial")),
                );
            }
            list = list.child(content);
        }
        list.flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .into_any_element()
    }

    pub(super) fn render(
        &self,
        id: RequestId,
        controller: &Entity<Controller>,
        context: &plugin::Context,
        cx: &App,
    ) -> AnyElement {
        let preview = self.view(SharedString::from(format!("notebook-preview-{id}")), cx);
        let mut view = v_flex().size_full().min_h_0().child(preview);
        if self.next.is_some() {
            let owner = controller.clone();
            let context = context.clone();
            view = view.child(
                Button::new("notebook-more")
                    .label(crate::tr("notebook_more"))
                    .ghost()
                    .loading(self.loading)
                    .disabled(self.loading)
                    .debug_selector(|| "notebook-more".into())
                    .on_click(move |_, window, cx| {
                        owner.update(cx, |owner, cx| {
                            owner.next_notebook(id, context.clone(), window, cx)
                        })
                    }),
            );
        }
        view.into_any_element()
    }
}

fn editor(state: &Entity<EditorState>, label: &str, cx: &App) -> AnyElement {
    let lines = state.read(cx).value().lines().count().clamp(1, 24);
    Editor::new(state)
        .readonly(true)
        .appearance(false)
        .bordered(false)
        .text_sm()
        .h(px(lines as f32 * 20. + 20.))
        .aria_label(label.to_owned())
        .into_any_element()
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            value
                .as_array()
                .map(|lines| lines.iter().filter_map(Value::as_str).collect())
        })
        .unwrap_or_default()
}

impl Controller {
    fn next_notebook(
        &mut self,
        id: RequestId,
        context: plugin::Context,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get_mut(&id) else {
            return;
        };
        let Some(state) = &mut document.notebook else {
            return;
        };
        let Some(offset) = state.next.filter(|_| !state.loading) else {
            return;
        };
        if document.input.read(cx).value().as_ref() != document.baseline {
            return;
        }
        state.loading = true;
        let revision = state.revision.clone();
        let path = document.path.clone();
        let stop = document.stop.clone();
        let client = self.binding.client.clone();
        let job = self.binding.runtime.spawn(async move {
            let worktree = context.worktree.ok_or_else(|| Fault::invalid("notebook requires a worktree"))?;
            let command = Command::ReadNotebook {worktree,options:notebook::Read {path,offset,limit:20}};
            let request = client.prepare(command).with_plugin(context);
            tokio::select! { _ = stop.cancelled() => Err(unavailable()), result = client.execute(request) => result }
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await.unwrap_or_else(|_| Err(unavailable()));
            let _ = owner.update_in(cx, |owner, window, cx| {
                let Some(document) = owner.documents.get_mut(&id) else {
                    return;
                };
                let Some(state) = &mut document.notebook else {
                    return;
                };
                state.loading = false;
                if state.revision != revision || state.next != Some(offset) {
                    return;
                }
                let result = match result {
                    Ok(Output::NotebookPage(page)) if page.revision == revision => {
                        state.append(page, window, cx);
                        Ok(())
                    }
                    Ok(Output::NotebookPage(_)) => Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "notebook changed while reading cells",
                    )),
                    Ok(_) => Err(Fault::invalid("invalid notebook page response")),
                    Err(fault) => Err(fault),
                };
                if let Err(fault) = result {
                    crate::feedback::error("", &fault.message, window, cx);
                }
                owner.changed(cx);
            });
        })
        .detach();
        self.changed(cx);
    }
}
