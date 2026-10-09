mod requests;
#[cfg(test)]
mod tests;
mod view;

use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    input::TextareaState,
};
use sailry_protocol::{
    WorktreeId,
    knowledge::{Definition, Source},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceKind {
    Directory,
    File,
    Worktree,
    Storage,
}

#[derive(Clone)]
pub(super) struct SourceChoice(SourceKind);

impl SearchableListItem for SourceChoice {
    type Value = SourceKind;
    fn title(&self) -> SharedString {
        tr(match self.0 {
            SourceKind::Directory => "knowledge_folder",
            SourceKind::File => "knowledge_file",
            SourceKind::Worktree => "knowledge_worktree",
            SourceKind::Storage => "knowledge_storage",
        })
    }
    fn value(&self) -> &Self::Value {
        &self.0
    }
}

#[derive(Clone)]
struct WorktreeChoice {
    id: WorktreeId,
    path: String,
}

impl SearchableListItem for WorktreeChoice {
    type Value = WorktreeId;
    fn title(&self) -> SharedString {
        self.path.clone().into()
    }
    fn value(&self) -> &Self::Value {
        &self.id
    }
}

#[derive(Clone)]
struct OverrideChoice {
    value: Option<Model>,
    label: SharedString,
}

impl SearchableListItem for OverrideChoice {
    type Value = Option<Model>;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub(super) struct Editor {
    owner: Entity<Panel>,
    collection: Option<Collection>,
    name: Entity<InputState>,
    pub(super) source: Entity<SelectState<Vec<SourceChoice>>>,
    worktree: Entity<SelectState<Vec<WorktreeChoice>>>,
    path: Entity<InputState>,
    model: Entity<SelectState<Vec<OverrideChoice>>>,
    globs: Entity<TextareaState>,
    tags: Entity<TextareaState>,
    shared: bool,
    pub(super) pending: bool,
    pub(super) closed: bool,
    request: Option<Request>,
    error: Option<&'static str>,
}

pub(super) fn open(
    owner: Entity<Panel>,
    collection: Option<Collection>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let worktrees = owner
        .read(cx)
        .worktrees
        .iter()
        .map(|tree| WorktreeChoice {
            id: tree.id,
            path: tree.path.clone(),
        })
        .collect::<Vec<_>>();
    let definition = collection.as_ref().map(|collection| &collection.definition);
    let (kind, path, worktree) = match definition.map(|definition| &definition.source) {
        Some(Source::Directory { path }) => (SourceKind::Directory, path.clone(), None),
        Some(Source::File { path }) => (SourceKind::File, path.clone(), None),
        Some(Source::Worktree { worktree, path }) => {
            (SourceKind::Worktree, path.clone(), Some(*worktree))
        }
        Some(Source::Storage { prefix, .. }) => (SourceKind::Storage, prefix.clone(), None),
        None => (SourceKind::Directory, String::new(), None),
    };
    let name = definition.map_or(String::new(), |definition| definition.name.clone());
    let model = definition.and_then(|definition| definition.model);
    let globs = definition.map_or(String::new(), |definition| definition.globs.join("\n"));
    let tags = definition.map_or(String::new(), |definition| definition.tags.join("\n"));
    let shared = definition.is_some_and(|definition| {
        definition
            .shared_with
            .iter()
            .any(|name| name == "knowledge")
    });
    let editor = cx.new(|cx| {
        crate::feedback::observe_with(
            window,
            cx,
            |editor: &Editor, _| editor.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("knowledge-editor-retry")
                        .debug_selector(|| "knowledge-editor-retry".into())
                        .label(tr(if key == "knowledge_unknown" {
                            "knowledge_retry"
                        } else {
                            "knowledge_reload"
                        }))
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |editor, cx| {
                                if key == "knowledge_unknown" {
                                    editor.save(window, cx);
                                } else {
                                    editor.reload(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        let source = cx.new(|cx| {
            let mut options = vec![
                SourceChoice(SourceKind::Directory),
                SourceChoice(SourceKind::File),
                SourceChoice(SourceKind::Worktree),
            ];
            if kind == SourceKind::Storage {
                options = vec![SourceChoice(SourceKind::Storage)];
            }
            let mut state = SelectState::new(options, None, window, cx);
            state.set_selected_value(&kind, window, cx);
            state
        });
        let worktree = cx.new(|cx| {
            let mut state = SelectState::new(worktrees, Some(IndexPath::new(0)), window, cx);
            if let Some(worktree) = worktree {
                state.set_selected_value(&worktree, window, cx);
            }
            state
        });
        let model = cx.new(|cx| {
            let mut options = std::iter::once(OverrideChoice {
                value: None,
                label: tr("knowledge_use_default"),
            })
            .chain(owner.read(cx).models.iter().map(|info| OverrideChoice {
                value: Some(info.model),
                label: model_label(info),
            }))
            .collect::<Vec<_>>();
            if let Some(model) = model
                && !options.iter().any(|option| option.value == Some(model))
            {
                options.push(OverrideChoice {
                    value: Some(model),
                    label: owner.read(cx).model_name(model),
                });
            }
            let mut state = SelectState::new(options, Some(IndexPath::new(0)), window, cx);
            state.set_selected_value(&model, window, cx);
            state
        });
        cx.observe_in(&source, window, |_, _, _, cx| cx.notify())
            .detach();
        Editor {
            owner,
            collection,
            name: cx.new(|cx| InputState::new(window, cx).default_value(name)),
            source,
            worktree,
            path: cx.new(|cx| InputState::new(window, cx).default_value(path)),
            model,
            globs: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 4)
                    .default_value(globs)
            }),
            tags: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 4)
                    .default_value(tags)
            }),
            shared,
            pending: false,
            closed: false,
            request: None,
            error: None,
        }
    });
    #[cfg(test)]
    owner_for_test(&editor, cx);
    let result = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let save = editor.clone();
        let cancel = editor.clone();
        let close = editor.clone();
        let locked = editor.read(cx).pending || editor.read(cx).request.is_some();
        dialog
            .form_title(tr(if editor.read(cx).collection.is_some() {
                "knowledge_edit_collection"
            } else {
                "knowledge_add_collection"
            }))
            .w((window.viewport_size().width - px(48.)).min(px(560.)))
            .overlay_closable(false)
            .keyboard(!locked)
            .close_button(!locked)
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| close.update(cx, |editor, _| editor.closed = true))
            .child(editor.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .child(
                        Button::new("knowledge-editor-cancel")
                            .debug_selector(|| "knowledge-editor-cancel".into())
                            .label(tr("settings_cancel"))
                            .disabled(locked)
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |editor, _| editor.closed = true);
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("knowledge-editor-save")
                            .primary()
                            .debug_selector(|| "knowledge-editor-save".into())
                            .label(tr(if editor.read(cx).request.is_some() {
                                "knowledge_retry"
                            } else {
                                "settings_save"
                            }))
                            .disabled(editor.read(cx).pending)
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| editor.save(window, cx))
                            }),
                    ),
            )
    });
    result
}

#[cfg(test)]
fn owner_for_test(editor: &Entity<Editor>, cx: &mut App) {
    let owner = editor.read(cx).owner.clone();
    owner.update(cx, |panel, _| panel.editor = Some(editor.downgrade()));
}

impl Editor {
    fn definition(&self, cx: &App) -> Result<Definition, &'static str> {
        let path = self.path.read(cx).value().trim().to_owned();
        let source = match self.source.read(cx).selected_value().copied() {
            Some(SourceKind::Directory) => Source::Directory { path },
            Some(SourceKind::File) => Source::File { path },
            Some(SourceKind::Worktree) => Source::Worktree {
                worktree: self
                    .worktree
                    .read(cx)
                    .selected_value()
                    .copied()
                    .ok_or("knowledge_invalid")?,
                path: if path.is_empty() { ".".into() } else { path },
            },
            Some(SourceKind::Storage) => self
                .collection
                .as_ref()
                .ok_or("knowledge_invalid")?
                .definition
                .source
                .clone(),
            None => return Err("knowledge_invalid"),
        };
        let mut shared_with = self.collection.as_ref().map_or(Vec::new(), |collection| {
            collection.definition.shared_with.clone()
        });
        shared_with.retain(|name| name != "knowledge");
        if self.shared {
            shared_with.push("knowledge".into());
        }
        let definition = Definition {
            name: self.name.read(cx).value().trim().to_owned(),
            source,
            model: self.model.read(cx).selected_value().copied().flatten(),
            globs: list(self.globs.read(cx).value().as_ref()),
            tags: list(self.tags.read(cx).value().as_ref()),
            shared_with,
        };
        definition.validate().map_err(|_| "knowledge_invalid")?;
        Ok(definition)
    }
}

fn list(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}
