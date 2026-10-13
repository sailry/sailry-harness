use super::*;
use gpui_kit::component::native_menu::NativeMenu;

fn outcome_key(outcomes: &[sailry_protocol::file_browser::Outcome]) -> Option<&'static str> {
    let errors: Vec<_> = outcomes
        .iter()
        .filter_map(|outcome| outcome.error.as_ref())
        .collect();
    if errors.iter().any(|error| {
        matches!(
            error.code,
            sailry_protocol::ErrorCode::OutcomeUnknown | sailry_protocol::ErrorCode::Unavailable
        )
    }) {
        Some("directory_outcome_unknown")
    } else if errors
        .iter()
        .any(|error| error.code == sailry_protocol::ErrorCode::Conflict)
    {
        Some("files_paste_conflict")
    } else if errors.is_empty() {
        None
    } else {
        Some("directory_action_failed")
    }
}

fn completion_toast(
    key: Option<&'static str>,
    target: sailry_protocol::NodeId,
    window: &mut Window,
    cx: &mut App,
) {
    let text: SharedString = format!(
        "{} · {}: {}",
        tr(key.unwrap_or("task_completed")),
        tr("project_host"),
        crate::live::node_key(target)
    )
    .into();
    let message = match key {
        Some(_) => Notification::error(text.clone()),
        None => Notification::success(text.clone()),
    };
    crate::feedback::toast(window, text, message, cx);
}

#[cfg(test)]
mod completion {
    use super::outcome_key;
    use sailry_protocol::{ErrorCode, Fault, file_browser::Outcome};

    #[core::prelude::v1::test]
    fn uncertainty_precedes_failures() {
        let outcome = |code| Outcome {
            path: "/fixture".into(),
            destination: None,
            error: Some(Fault::new(code, "fixture")),
        };
        for code in [ErrorCode::OutcomeUnknown, ErrorCode::Unavailable] {
            assert_eq!(
                outcome_key(&[outcome(ErrorCode::Conflict), outcome(code)]),
                Some("directory_outcome_unknown")
            );
        }
        assert_eq!(
            outcome_key(&[outcome(ErrorCode::Conflict)]),
            Some("files_paste_conflict")
        );
        assert_eq!(outcome_key(&[]), None);
    }
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = file_picker, no_json)]
pub(crate) struct Dispatch(pub(crate) Operation);

#[derive(Clone, PartialEq)]
pub(crate) enum Operation {
    Open,
    Copy,
    Cut,
    Paste(Conflict),
    Rename,
    Create,
    Trash,
    Refresh,
}

impl Picker {
    pub(super) fn menu(
        &mut self,
        index: Option<usize>,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self.closed || self.busy || !(self.valid)(cx) {
            return;
        }
        if index.is_some_and(|index| {
            !matches!(
                self.entries()[index].kind,
                EntryKind::File | EntryKind::Directory
            )
        }) {
            return;
        }
        self.focus.focus(window, cx);
        if let Some(index) = index
            && !self.selected.contains(&index)
        {
            self.choose(index, false, window, cx);
        }
        if let Some(index) = index {
            self.focused = Some(index);
        }
        self.menu_directory = index
            .filter(|index| self.entries()[*index].kind == EntryKind::Directory)
            .map(|index| self.entry_path(index))
            .or_else(|| {
                index
                    .is_none()
                    .then(|| {
                        self.listing
                            .as_ref()
                            .map(|listing| listing.directory.path.clone())
                    })
                    .flatten()
            });
        let action = |operation| Box::new(Dispatch(operation)) as Box<dyn gpui_kit::Action>;
        let mut menu = NativeMenu::new();
        if let Some(index) = index {
            menu = menu
                .menu_with_disabled(
                    tr("directory_open"),
                    self.entries()[index].kind != EntryKind::Directory
                        && !matches!(self.mode, Mode::Files { .. }),
                    action(Operation::Open),
                )
                .separator()
                .menu(tr("files_copy"), action(Operation::Copy))
                .menu(tr("files_cut"), action(Operation::Cut))
                .menu_with_disabled(
                    tr("files_rename"),
                    self.selected.len() != 1,
                    action(Operation::Rename),
                );
        } else {
            menu = menu.menu(tr("directory_create"), action(Operation::Create));
        }
        let disabled = self.clipboard.is_none() || self.menu_directory.is_none();
        menu = menu.submenu(
            tr("files_paste"),
            NativeMenu::new()
                .menu_with_disabled(
                    tr("directory_stop"),
                    disabled,
                    action(Operation::Paste(Conflict::Stop)),
                )
                .menu_with_disabled(
                    tr("files_upload_replace"),
                    disabled,
                    action(Operation::Paste(Conflict::Replace)),
                )
                .menu_with_disabled(
                    tr("directory_keep_both"),
                    disabled,
                    action(Operation::Paste(Conflict::KeepBoth)),
                ),
        );
        if index.is_some() {
            menu = menu
                .separator()
                .menu(tr("files_trash"), action(Operation::Trash));
        } else {
            menu = menu
                .separator()
                .menu(tr("directory_refresh"), action(Operation::Refresh));
        }
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            menu.show(event.position, window, cx);
        } else {
            self.fail("workspace_native_menu_unavailable", window, cx);
        }
    }

    pub(super) fn dispatch(
        &mut self,
        action: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed || self.busy || !(self.valid)(cx) {
            return;
        }
        match action.0 {
            Operation::Open => {
                if let Some(index) = self.focused {
                    self.activate(index, window, cx);
                }
            }
            Operation::Copy => self.copy(false),
            Operation::Cut => self.copy(true),
            Operation::Paste(conflict) => self.paste(conflict, window, cx),
            Operation::Rename => self.name(true, window, cx),
            Operation::Create => self.name(false, window, cx),
            Operation::Trash => self.trash(window, cx),
            Operation::Refresh => self.refresh(window, cx),
        }
    }

    pub(super) fn copy(&mut self, cut: bool) {
        let paths: Vec<_> = self
            .selected
            .iter()
            .map(|index| self.entry_path(*index))
            .collect();
        // The clipboard belongs to this picker and therefore cannot cross hosts.
        if !paths.is_empty() {
            self.clipboard = Some((paths, cut));
        }
    }

    pub(super) fn paste(
        &mut self,
        conflict: Conflict,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((paths, cut)) = self.clipboard.clone() else {
            return;
        };
        let Some(destination) = self.menu_directory.take().or_else(|| {
            self.listing
                .as_ref()
                .map(|listing| listing.directory.path.clone())
        }) else {
            return;
        };
        self.execute(
            FileAction::Transfer {
                paths,
                destination,
                cut,
                conflict,
            },
            window,
            cx,
        );
    }

    fn name(&mut self, rename: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected.iter().next().copied();
        if rename && (selected.is_none() || self.selected.len() != 1) {
            return;
        }
        let Some(listing) = &self.listing else {
            return;
        };
        let parent = listing.directory.path.clone();
        let from = selected
            .filter(|_| rename)
            .map(|index| self.entry_path(index));
        let value = selected
            .filter(|_| rename)
            .map(|index| self.entries()[index].name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value)
                .placeholder(tr("directory_name"))
        });
        let picker = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let picker = picker.clone();
            let input = input.clone();
            let from = from.clone();
            let parent = parent.clone();
            dialog
                .w_96()
                .form_title(tr(if rename {
                    "files_rename"
                } else {
                    "directory_create"
                }))
                .child(
                    div()
                        .debug_selector(|| "directory-name".into())
                        .child(Input::new(&input)),
                )
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text(tr("settings_save"))
                        .cancel_text(tr("settings_cancel")),
                )
                .footer(
                    gpui_kit::component::dialog::DialogFooter::new()
                        .child(
                            gpui_kit::component::dialog::DialogClose::new().trigger(|button| {
                                button
                                    .label(tr("settings_cancel"))
                                    .debug_selector(|| "directory-name-cancel".into())
                            }),
                        )
                        .child(
                            gpui_kit::component::dialog::DialogAction::new().child(
                                Button::new("directory-name-save")
                                    .primary()
                                    .label(tr("settings_save"))
                                    .debug_selector(|| "directory-name-save".into()),
                            ),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value();
                    let name = name.trim();
                    if name.is_empty()
                        || matches!(name, "." | "..")
                        || name.contains(['/', '\\', '\0'])
                    {
                        crate::feedback::toast(
                            window,
                            tr("directory_invalid_name"),
                            Notification::error(tr("directory_invalid_name")),
                            cx,
                        );
                        return false;
                    }
                    let _ = picker.update(cx, |picker, cx| {
                        let to = picker.join(&parent, name);
                        let action = match &from {
                            Some(from) => FileAction::Rename {
                                from: from.clone(),
                                to,
                            },
                            None => FileAction::CreateDirectory { path: to },
                        };
                        picker.execute(action, window, cx);
                    });
                    true
                })
        });
    }

    fn trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<_> = self
            .selected
            .iter()
            .map(|index| self.entry_path(*index))
            .collect();
        if paths.is_empty() {
            return;
        }
        let names = self
            .selected
            .iter()
            .map(|index| self.entries()[*index].name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let picker = cx.entity().downgrade();
        crate::prompts::confirm(
            &tr("files_trash"),
            &rust_i18n::t!("files_trash_confirm", name = names),
            tr("files_trash"),
            window,
            cx,
            move |window, cx| {
                let _ = picker.update(cx, |picker, cx| {
                    picker.execute(FileAction::Trash { paths }, window, cx)
                });
            },
        );
    }

    fn execute(&mut self, action: FileAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.busy || !(self.valid)(cx) {
            return;
        }
        self.cancel_children();
        let Some(client) = self.client.clone() else {
            match self.catalog.apply(&action) {
                Ok(()) => {
                    if matches!(action, FileAction::Transfer { cut: true, .. }) {
                        self.clipboard = None;
                    }
                    self.refresh(window, cx);
                }
                Err(error) => self.fail(error, window, cx),
            }
            return;
        };
        let command = Command::ManageFiles(action);
        if let Some(request) = &self.request
            && request.command != command
        {
            self.fail("directory_outcome_unknown", window, cx);
            return;
        }
        let request = self
            .request
            .get_or_insert_with(|| client.prepare(command))
            .clone();
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let generation = self.generation;
        let target = client.target();
        let job = runtime.spawn(async move { client.execute(request).await });
        self.busy = true;
        cx.spawn_in(window, async move |picker, cx| {
            let result = job.await;
            let key = match &result {
                Ok(Ok(Output::FilesManaged(outcomes))) => outcome_key(outcomes),
                Ok(Err(error))
                    if !matches!(
                        error.code,
                        sailry_protocol::ErrorCode::OutcomeUnknown
                            | sailry_protocol::ErrorCode::Unavailable
                    ) =>
                {
                    Some("directory_action_failed")
                }
                _ => Some("directory_outcome_unknown"),
            };
            let updated = picker.update_in(cx, |picker, window, cx| {
                if picker.closed || picker.generation != generation || !(picker.valid)(cx) {
                    completion_toast(key, target, window, cx);
                    // Presentation is gone, but the captured request remains available
                    // for outcome reconciliation. Never rebind or automatically replay it.
                    return;
                }
                picker.busy = false;
                match result {
                    Ok(Ok(Output::FilesManaged(outcomes))) => {
                        if key != Some("directory_outcome_unknown") {
                            picker.request = None;
                        }
                        if let Some((paths, true)) = &mut picker.clipboard {
                            paths.retain(|path| {
                                !outcomes
                                    .iter()
                                    .any(|outcome| &outcome.path == path && outcome.error.is_none())
                            });
                            if paths.is_empty() {
                                picker.clipboard = None;
                            }
                        }
                        if let Some(key) = key {
                            picker.fail(key, window, cx);
                        }
                        picker.refresh(window, cx);
                    }
                    Ok(Err(error))
                        if !matches!(
                            error.code,
                            sailry_protocol::ErrorCode::OutcomeUnknown
                                | sailry_protocol::ErrorCode::Unavailable
                        ) =>
                    {
                        picker.request = None;
                        picker.fail("directory_action_failed", window, cx);
                    }
                    _ => picker.fail("directory_outcome_unknown", window, cx),
                }
                cx.notify();
            });
            if updated.is_err() {
                let _ = cx.update(|window, cx| completion_toast(key, target, window, cx));
            }
        })
        .detach();
        cx.notify();
    }
}
