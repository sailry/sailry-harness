//! Project registration adapts the shared chooser without owning its UI.
use super::*;
use crate::file_picker::{self, Completion, Mode, Options, Picker, Selection};
use sailry_client::Client;
use std::sync::Arc;

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
    let target = client.as_ref().map(|client| client.target());
    let source = editor.downgrade();
    let retained = register.then(|| editor.clone());
    let callback_original = original.clone();
    let callback_source = source.clone();
    let callback_target = target;
    let picker = file_picker::open(
        Options {
            client,
            preview_host: host,
            initial: (!original.trim().is_empty()).then(|| original.to_string()),
            mode: Mode::Directory,
            title: if register {
                "menu_open_folder"
            } else {
                "directory_title"
            },
            confirm: if register {
                "directory_open"
            } else {
                "directory_title"
            },
        },
        move |selection, window, cx| {
            let _retained = &retained;
            let Selection::Directory(path) = selection else {
                return Err("directory_stale");
            };
            callback_source
                .update(cx, |editor, cx| {
                    if editor.host != host
                        || editor.live.as_ref().map(|transport| transport.target())
                            != callback_target
                        || !register && editor.inputs[1].read(cx).value() != callback_original
                    {
                        return Err("directory_stale");
                    }
                    editor.inputs[1]
                        .update(cx, |input, cx| input.set_value(path.clone(), window, cx));
                    editor.suggest(window, cx);
                    editor.error = None;
                    if register && !editor.open_registered(&path, window, cx) {
                        editor.save(window, cx);
                    }
                    cx.notify();
                    Ok(if register {
                        Completion::KeepOpen
                    } else {
                        Completion::Close
                    })
                })
                .unwrap_or(Err("directory_stale"))
        },
        window,
        cx,
    );
    let pending = source.clone();
    picker.update(cx, |picker, cx| {
        picker.bind(
            move |cx| {
                source.upgrade().is_some_and(|editor| {
                    let editor = editor.read(cx);
                    editor.host == host
                        && editor.live.as_ref().map(|transport| transport.target()) == target
                        && (register || editor.inputs[1].read(cx).value() == original)
                })
            },
            move |cx| {
                pending
                    .upgrade()
                    .is_some_and(|editor| editor.read(cx).pending)
            },
        );
        let observer = cx.observe_in(&editor, window, |picker, _, _, cx| picker.invalidate(cx));
        picker.observe(observer);
    });
    picker
}
