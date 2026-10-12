//! Shared execution-host file chooser. Save returns a path and never writes it.
//! Feature provenance: Sailry Code d9b56405, harbor_file_picker.dart.
//! Kit tables track one focused row. Multi-file selection uses a Kit name-cell
//! checkbox; the application also composes grid selection from Kit buttons.
pub(crate) mod actions;
mod catalog;
mod navigation;
mod render;
mod selection;
#[cfg(test)]
mod tests;
use crate::{theme::DialogStyle as _, tr};
use catalog::Catalog;
use gpui_kit::component::{
    VirtualListScrollHandle,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    notification::Notification,
    table::TableState,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
pub(crate) use navigation::Visit;
use sailry_client::Client;
use sailry_protocol::{
    Command, EntryKind, FileEntry, Output, Request,
    file_browser::{Action as FileAction, Conflict, Listing},
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mode {
    Directory,
    Files { multiple: bool },
    Save { suggested_name: String },
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Selection {
    Directory(String),
    Files(Vec<String>),
    Save(String),
}
pub(crate) enum Completion {
    Close,
    KeepOpen,
}
pub(crate) struct Options {
    pub client: Option<Arc<Client>>,
    pub preview_host: usize,
    pub initial: Option<String>,
    pub mode: Mode,
    pub title: &'static str,
    pub confirm: &'static str,
}
type Callback =
    Box<dyn FnMut(Selection, &mut Window, &mut App) -> Result<Completion, &'static str>>;
type Guard = Box<dyn Fn(&App) -> bool>;

pub(crate) fn open(
    options: Options,
    callback: impl FnMut(Selection, &mut Window, &mut App) -> Result<Completion, &'static str> + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Picker> {
    let title = options.title;
    let initial = options.initial;
    let picker = cx.new(|cx| {
        let address = cx.new(|cx| InputState::new(window, cx).placeholder(tr("directory_address")));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(tr("directory_filter")));
        let filename = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("directory_name"))
                .default_value(match &options.mode {
                    Mode::Save { suggested_name } => suggested_name.clone(),
                    _ => String::new(),
                })
        });
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe_in(
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
                if matches!(event, InputEvent::Blur) {
                    picker.editing_address = false;
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &filter,
            window,
            |picker: &mut Picker, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    picker.rebuild(cx);
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &filename,
            window,
            |picker: &mut Picker, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && matches!(picker.mode, Mode::Save { .. })
                {
                    picker.confirm(window, cx);
                }
                if matches!(event, InputEvent::Change)
                    && (picker.name_error || picker.error == Some("directory_invalid_name"))
                    && (picker.valid)(cx)
                    && picker.request.is_none()
                    && !matches!(
                        picker.error,
                        Some("directory_stale" | "directory_outcome_unknown")
                    )
                {
                    picker.error = None;
                    picker.name_error = false;
                }
                cx.notify();
            },
        ));
        let preference = crate::preferences::data(cx).file_picker.unwrap_or_default();
        let table =
            cx.new(|cx| TableState::new(render::Rows::new(), window, cx).row_selectable(false));
        let mut picker = Picker {
            catalog: Catalog::new(options.preview_host),
            client: options.client,
            mode: options.mode,
            listing: None,
            address,
            filter,
            filename,
            editing_address: false,
            grid: preference.grid,
            hidden: preference.hidden,
            visible: Vec::new(),
            history: Vec::new(),
            cursor: 0,
            selected: BTreeSet::new(),
            focused: None,
            anchor: None,
            focus: cx.focus_handle(),
            columns: 1,
            scroll: VirtualListScrollHandle::new(),
            locations_scroll: ScrollHandle::new(),
            table,
            clipboard: None,
            menu_directory: None,
            error: None,
            name_error: false,
            busy: false,
            request: None,
            task: None,
            read_stop: None,
            generation: 0,
            callback: Some(Box::new(callback)),
            valid: Box::new(|_| true),
            pending: Box::new(|_| false),
            confirm: options.confirm,
            subscriptions,
            overlay: Default::default(),
            closed: false,
        };
        let owner = cx.entity().downgrade();
        picker
            .table
            .update(cx, |table, _| table.delegate_mut().owner = Some(owner));
        picker.navigate(initial, Visit::Push, window, cx);
        picker
    });
    let result = picker.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let cancel = picker.clone();
        let close = picker.clone();
        dialog
            .margin_top(px(24.))
            .form_title(tr(title))
            .w((window.viewport_size().width - px(48.)).min(px(900.)))
            .overlay_closable(false)
            .on_ok(|_, _, _| false)
            .on_cancel(move |_, _, cx| {
                cancel.update(cx, |picker, _| picker.stop());
                true
            })
            .on_close(move |_, _, cx| {
                close.update(cx, |picker, _| {
                    picker.stop();
                    picker.overlay.dismissed();
                })
            })
            .child(picker.clone())
            .footer(picker.read(cx).footer(picker.clone(), window, cx))
    });
    result.update(cx, |picker, cx| {
        crate::ui::overlays::Owned::bind(picker, |picker| &mut picker.overlay, window, cx)
    });
    result
}

pub(crate) struct Picker {
    catalog: Catalog,
    client: Option<Arc<Client>>,
    mode: Mode,
    listing: Option<Listing>,
    address: Entity<InputState>,
    filter: Entity<InputState>,
    filename: Entity<InputState>,
    editing_address: bool,
    grid: bool,
    hidden: bool,
    visible: Vec<usize>,
    history: Vec<String>,
    cursor: usize,
    pub(crate) selected: BTreeSet<usize>,
    focused: Option<usize>,
    anchor: Option<usize>,
    focus: FocusHandle,
    columns: usize,
    scroll: VirtualListScrollHandle,
    locations_scroll: ScrollHandle,
    table: Entity<TableState<render::Rows>>,
    pub(crate) clipboard: Option<(Vec<String>, bool)>,
    menu_directory: Option<String>,
    error: Option<&'static str>,
    name_error: bool,
    pub(crate) busy: bool,
    request: Option<Request>,
    task: Option<Task<()>>,
    read_stop: Option<sailry_link::CancellationToken>,
    generation: u64,
    callback: Option<Callback>,
    valid: Guard,
    pending: Guard,
    confirm: &'static str,
    subscriptions: Vec<Subscription>,
    overlay: crate::ui::overlays::Owned,
    closed: bool,
}
impl Drop for Picker {
    fn drop(&mut self) {
        if let Some(stop) = &self.read_stop {
            stop.cancel();
        }
    }
}
impl Picker {
    pub(crate) fn bind(
        &mut self,
        valid: impl Fn(&App) -> bool + 'static,
        pending: impl Fn(&App) -> bool + 'static,
    ) {
        self.valid = Box::new(valid);
        self.pending = Box::new(pending);
    }
    pub(crate) fn observe(&mut self, subscription: Subscription) {
        self.subscriptions.push(subscription);
    }
    pub(crate) fn invalidate(&mut self, cx: &mut Context<Self>) {
        if !(self.valid)(cx) {
            self.generation += 1;
            if let Some(stop) = self.read_stop.take() {
                stop.cancel();
            }
            self.busy = false;
            self.error = Some("directory_stale");
        }
        cx.notify();
    }
    pub(crate) fn entries(&self) -> &[FileEntry] {
        self.listing
            .as_ref()
            .map(|value| value.directory.entries.as_slice())
            .unwrap_or_default()
    }
    fn join(&self, directory: &str, name: &str) -> String {
        let separator = self
            .listing
            .as_ref()
            .map(|value| value.separator)
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
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.visible =
            selection::visible(self.entries(), self.hidden, &self.filter.read(cx).value());
        self.selected.retain(|index| self.visible.contains(index));
        self.focused = self.focused.filter(|index| self.visible.contains(index));
        self.anchor = self.anchor.filter(|index| self.visible.contains(index));
        self.sync_table(cx);
    }
    fn sync_table(&self, cx: &mut Context<Self>) {
        let entries: Vec<_> = self
            .visible
            .iter()
            .map(|index| (*index, self.entries()[*index].clone()))
            .collect();
        let selected = self.selected.clone();
        let filtered = !self.filter.read(cx).value().is_empty();
        let multiple = matches!(self.mode, Mode::Files { multiple: true });
        self.table.update(cx, |table, cx| {
            let rows = table.delegate_mut();
            rows.entries = entries;
            rows.selected = selected;
            rows.filtered = filtered;
            rows.multiple = multiple;
            cx.notify();
        });
    }
    fn selection(&self, cx: &App) -> Option<Selection> {
        selection::resolve(
            &self.mode,
            self.listing.as_ref()?,
            &self.selected,
            &self.filename.read(cx).value(),
        )
    }
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.busy || (self.pending)(cx) || self.error.is_some() {
            return;
        }
        if !(self.valid)(cx) {
            self.fail("directory_stale", window, cx);
            return;
        }
        let Some(selection) = self.selection(cx) else {
            self.fail("directory_invalid_name", window, cx);
            return;
        };
        if self.client.is_none()
            && let Selection::Directory(path) = &selection
            && let Err(error) = self.catalog.list(path)
        {
            self.fail(error, window, cx);
            return;
        }
        let Some(mut callback) = self.callback.take() else {
            return;
        };
        let result = callback(selection, window, cx);
        self.callback = Some(callback);
        match result {
            Ok(Completion::Close) => self.close(window, cx),
            Ok(Completion::KeepOpen) => cx.notify(),
            Err(error) => {
                self.fail(error, window, cx);
                self.name_error = true;
            }
        }
    }
    fn close(&mut self, window: &mut Window, cx: &mut App) {
        self.stop();
        self.overlay.close(window, cx);
    }
    fn stop(&mut self) {
        self.closed = true;
        self.generation += 1;
        if let Some(stop) = self.read_stop.take() {
            stop.cancel();
            self.busy = false;
        }
    }
    fn fail(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.error = Some(key);
        self.name_error = false;
        crate::feedback::toast(
            window,
            tr(key),
            Notification::error(tr(key)).id::<Self>(),
            cx,
        );
        cx.notify();
    }
    fn preference(&self, cx: &mut App) {
        crate::preferences::update(cx, |data| {
            data.file_picker = Some(crate::preferences::FilePicker {
                grid: self.grid,
                hidden: self.hidden,
            })
        });
    }
}
