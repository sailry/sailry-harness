//! File picker behavior: Sailry Code d9b56405, harbor_file_picker.dart.
//! Compose the historical layout with Kit controls, not Flutter styling.
mod actions;
mod catalog;
mod navigation;
mod render;
#[cfg(test)]
mod tests;

use super::*;
use catalog::Catalog;
use gpui_kit::component::{VirtualListScrollHandle, input::InputEvent, notification::Notification};
use navigation::Visit;
use sailry_client::Client;
use sailry_protocol::{
    Command, EntryKind, FileEntry, Output, Request,
    file_browser::{Action as FileAction, Conflict, Listing},
};
use std::{collections::BTreeSet, sync::Arc};

pub(super) fn open(editor: Entity<Editor>, window: &mut Window, cx: &mut App) -> Entity<Picker> {
    open_with(editor, false, window, cx)
}

pub(super) fn open_project(
    editor: Entity<Editor>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Picker> {
    open_with(editor, true, window, cx)
}

fn open_with(
    editor: Entity<Editor>,
    register: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Picker> {
    let host = editor.read(cx).host;
    let original = editor.read(cx).inputs[1].read(cx).value();
    let client = editor.read(cx).live.clone().map(Client::new).map(Arc::new);
    let picker = cx.new(|cx| {
        if register {
            cx.observe_in(&editor, window, |_, _, _, cx| cx.notify())
                .detach();
        }
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(original.clone())
                .placeholder(tr("directory_address"))
        });
        let subscription = cx.subscribe_in(
            &address,
            window,
            |picker: &mut Picker, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    picker.navigate(
                        Some(input.read(cx).value().trim().to_owned()),
                        Visit::Push,
                        window,
                        cx,
                    );
                }
            },
        );
        let mut picker = Picker {
            editor: editor.downgrade(),
            registration: register.then(|| editor.clone()),
            original: original.clone(),
            host,
            catalog: Catalog::new(host),
            listing: client
                .is_none()
                .then(|| Catalog::new(host).list("/preview").unwrap()),
            client,
            address,
            history: Vec::new(),
            cursor: 0,
            selected: BTreeSet::new(),
            focused: None,
            focus: cx.focus_handle(),
            columns: 1,
            scroll: VirtualListScrollHandle::new(),
            locations_scroll: ScrollHandle::new(),
            clipboard: None,
            menu_directory: None,
            error: None,
            busy: false,
            request: None,
            task: None,
            _subscription: subscription,
        };
        picker.navigate(
            (!original.trim().is_empty()).then(|| original.to_string()),
            Visit::Push,
            window,
            cx,
        );
        picker
    });
    let result = picker.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let confirm = picker.clone();
        dialog
            .margin_top(px(24.))
            .form_title(tr(if register {
                "menu_open_folder"
            } else {
                "directory_title"
            }))
            .w((window.viewport_size().width - px(48.)).min(px(900.)))
            .overlay_closable(false)
            .on_ok(|_, _, _| false)
            .child(picker.clone())
            .footer(
                gpui_kit::component::dialog::DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("directory-cancel")
                            .label(tr("settings_cancel"))
                            .debug_selector(|| "directory-cancel".into())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("directory-confirm")
                            .primary()
                            .label(tr(if register {
                                "directory_open"
                            } else {
                                "directory_title"
                            }))
                            .loading(
                                picker
                                    .read(cx)
                                    .registration
                                    .as_ref()
                                    .is_some_and(|editor| editor.read(cx).pending),
                            )
                            .disabled(
                                picker.read(cx).busy
                                    || picker
                                        .read(cx)
                                        .registration
                                        .as_ref()
                                        .is_some_and(|editor| editor.read(cx).pending)
                                    || picker.read(cx).listing.is_none()
                                    || picker.read(cx).error.is_some(),
                            )
                            .debug_selector(|| "directory-confirm".into())
                            .on_click(move |_, window, cx| {
                                confirm.update(cx, |picker, cx| picker.confirm(window, cx))
                            }),
                    ),
            )
    });
    result
}

pub(super) struct Picker {
    editor: WeakEntity<Editor>,
    registration: Option<Entity<Editor>>,
    original: SharedString,
    host: usize,
    catalog: Catalog,
    client: Option<Arc<Client>>,
    listing: Option<Listing>,
    address: Entity<InputState>,
    history: Vec<String>,
    cursor: usize,
    selected: BTreeSet<usize>,
    focused: Option<usize>,
    focus: FocusHandle,
    columns: usize,
    scroll: VirtualListScrollHandle,
    locations_scroll: ScrollHandle,
    clipboard: Option<(Vec<String>, bool)>,
    menu_directory: Option<String>,
    error: Option<&'static str>,
    busy: bool,
    request: Option<Request>,
    task: Option<Task<()>>,
    _subscription: Subscription,
}

impl Picker {
    fn entries(&self) -> &[FileEntry] {
        self.listing
            .as_ref()
            .map(|listing| listing.directory.entries.as_slice())
            .unwrap_or_default()
    }

    fn join(&self, directory: &str, name: &str) -> String {
        let separator = self
            .listing
            .as_ref()
            .map(|listing| listing.separator)
            .unwrap_or('/');
        format!(
            "{}{}{}",
            directory.trim_end_matches(separator),
            separator,
            name
        )
    }

    fn entry_path(&self, index: usize) -> String {
        self.join(
            &self.listing.as_ref().unwrap().directory.path,
            &self.entries()[index].name,
        )
    }

    fn selection(&self) -> Option<String> {
        self.selected
            .iter()
            .find(|index| self.entries()[**index].kind == EntryKind::Directory)
            .map(|index| self.entry_path(*index))
            .or_else(|| {
                self.listing
                    .as_ref()
                    .map(|listing| listing.directory.path.clone())
            })
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy
            || self.error.is_some()
            || self
                .registration
                .as_ref()
                .is_some_and(|editor| editor.read(cx).pending)
        {
            return;
        }
        let Some(path) = self.selection() else {
            return;
        };
        if self.client.is_none()
            && let Err(error) = self.catalog.list(&path)
        {
            self.fail(error, window, cx);
            return;
        }
        let applied = self
            .editor
            .update(cx, |editor, cx| {
                if editor.host != self.host
                    || editor.live.as_ref().map(|transport| transport.target())
                        != self.client.as_ref().map(|client| client.target())
                    || self.registration.is_none()
                        && editor.inputs[1].read(cx).value() != self.original
                {
                    return false;
                }
                editor.inputs[1].update(cx, |input, cx| input.set_value(path.clone(), window, cx));
                editor.suggest(window, cx);
                editor.error = None;
                if self.registration.is_some() && !editor.open_registered(&path, window, cx) {
                    editor.save(window, cx);
                }
                cx.notify();
                true
            })
            .unwrap_or(false);
        if applied && self.registration.is_none() {
            window.close_dialog(cx);
        } else if !applied {
            self.fail("directory_stale", window, cx);
        }
    }

    fn fail(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.error = Some(key);
        crate::feedback::toast(
            window,
            tr(key),
            Notification::error(tr(key)).id::<Self>(),
            cx,
        );
        cx.notify();
    }
}
