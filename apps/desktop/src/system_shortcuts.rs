//! Native global shortcuts activate the existing controller and capture explicit selections.
#[cfg(not(test))]
mod selection;
#[cfg(test)]
mod tests;
use crate::shell::Shell;
#[cfg(not(test))]
use crate::shell::closing;
#[cfg(not(test))]
use crate::tr;
#[cfg(not(test))]
use futures::{StreamExt, channel::mpsc};
#[cfg(not(test))]
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
#[cfg(not(test))]
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::*;
use std::collections::BTreeMap;

struct Service {
    manager: GlobalHotKeyManager,
    keys: BTreeMap<String, HotKey>,
}
impl Global for Service {}
impl Drop for Service {
    fn drop(&mut self) {
        for key in self.keys.values() {
            let _ = self.manager.unregister(*key);
        }
    }
}

#[derive(Clone)]
#[cfg(not(test))]
struct Composer {
    view: Entity<crate::conversation::live::View>,
    node: sailry_protocol::NodeId,
    worktree: Option<sailry_protocol::WorktreeId>,
    session: Option<sailry_protocol::SessionId>,
}

#[cfg(not(test))]
impl Composer {
    fn capture(view: Entity<crate::conversation::live::View>, cx: &App) -> Self {
        let binding = view.read(cx).binding();
        Self {
            node: binding.client.target(),
            worktree: binding.worktree,
            session: view.read(cx).session(),
            view,
        }
    }

    fn insert(&self, text: &str, window: &mut Window, cx: &mut App) -> bool {
        let binding = self.view.read(cx).binding();
        binding.client.target() == self.node
            && binding.worktree == self.worktree
            && self.view.read(cx).session() == self.session
            && self
                .view
                .update(cx, |view, cx| view.insert_quote(text, window, cx))
    }
}

pub(crate) fn install(window: &mut Window, cx: &mut Context<Shell>) {
    // Preview controllers do not acquire native resources or inspect other apps.
    if cx.try_global::<crate::backend::Services>().is_none() || cx.has_global::<Service>() {
        return;
    }
    #[cfg(test)]
    {
        let _ = window;
    }
    #[cfg(not(test))]
    {
        let Ok(manager) = GlobalHotKeyManager::new() else {
            crate::feedback::error("", &tr("shortcut_unavailable"), window, cx);
            return;
        };
        cx.set_global(Service {
            manager,
            keys: BTreeMap::new(),
        });
        let (sender, mut receiver) = mpsc::unbounded();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                let _ = sender.unbounded_send(event.id);
            }
        }));
        cx.spawn(async move |_, cx| {
            while let Some(id) = receiver.next().await {
                cx.update(|cx| {
                    let key = cx
                        .global::<Service>()
                        .keys
                        .iter()
                        .find(|(_, key)| key.id() == id)
                        .map(|(name, _)| name.clone());
                    if let Some(key) = key {
                        dispatch(&key, cx);
                    }
                });
            }
        })
        .detach();
        for entry in crate::shortcuts::entries(cx)
            .into_iter()
            .filter(|entry| entry.id.starts_with("system."))
        {
            if configure(entry.id, &entry.key, cx).is_err() {
                crate::feedback::error("", &tr("shortcut_conflict"), window, cx);
            }
        }
    }
}

fn parse(value: &str) -> Result<Option<HotKey>, &'static str> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.split_whitespace().count() != 1 {
        return Err("shortcut_invalid");
    }
    let tokens = value
        .split('-')
        .map(|token| match token {
            "secondary" => "CommandOrControl",
            "platform" | "cmd" => "Super",
            "ctrl" => "Control",
            "space" => "Space",
            token => token,
        })
        .collect::<Vec<_>>();
    let key: HotKey = tokens.join("+").parse().map_err(|_| "shortcut_invalid")?;
    if key.mods.is_empty() {
        return Err("shortcut_invalid");
    }
    Ok(Some(key))
}

pub(crate) fn configure(id: &str, value: &str, cx: &mut App) -> Result<(), &'static str> {
    let key = parse(value)?;
    if !cx.has_global::<Service>() {
        return if cx.has_global::<crate::backend::Services>() {
            Err("shortcut_unavailable")
        } else {
            Ok(())
        };
    }
    let service = cx.global_mut::<Service>();
    if service.keys.get(id).copied() == key {
        return Ok(());
    }
    if let Some(key) = key {
        service
            .manager
            .register(key)
            .map_err(|_| "shortcut_conflict")?;
    }
    if let Some(old) = service.keys.remove(id)
        && service.manager.unregister(old).is_err()
    {
        if let Some(key) = key {
            let _ = service.manager.unregister(key);
        }
        service.keys.insert(id.into(), old);
        return Err("shortcut_unavailable");
    }
    if let Some(key) = key {
        service.keys.insert(id.into(), key);
    }
    Ok(())
}

#[cfg(not(test))]
fn dispatch(id: &str, cx: &mut App) {
    let Some((handle, owner)) = closing::front(cx) else {
        return;
    };
    if id == "system.activate" {
        cx.activate(true);
        let _ = handle.update(cx, |_, window, cx| {
            window.activate_window();
            let _ = owner.update(cx, |shell, cx| {
                shell.search(&crate::shell::Search, window, cx)
            });
        });
    } else if id == "system.selection" {
        let _ = handle.update(cx, |_, window, cx| {
            let chat = owner
                .read_with(cx, |shell, cx| {
                    shell
                        .current_chat()
                        .cloned()
                        .map(|view| Composer::capture(view, cx))
                })
                .ok()
                .flatten();
            let capture = cx.background_executor().spawn(async { selection::read() });
            window
                .spawn(cx, async move |cx| {
                    let text = capture.await;
                    let _ = cx.update(|window, cx| {
                        cx.activate(true);
                        window.activate_window();
                        match text {
                            Ok(text) if !text.trim().is_empty() => show(text, chat, window, cx),
                            Err(key) => crate::feedback::error("", &tr(key), window, cx),
                            _ => crate::feedback::error("", &tr("selection_empty"), window, cx),
                        }
                    });
                })
                .detach();
        });
    }
}

#[cfg(not(test))]
fn show(text: String, chat: Option<Composer>, window: &mut Window, cx: &mut App) {
    use crate::theme::DialogStyle as _;
    if window.has_active_dialog(cx) {
        return;
    }
    let id = format!("selection-{}", sailry_protocol::RequestId::new());
    window.open_dialog(cx, move |dialog, _, cx| {
        let spoken = text.clone();
        let quote = text.clone();
        let chat = chat.clone();
        let id = id.clone();
        dialog
            .form_title(tr("selection_title"), cx)
            .child(div().max_h(px(260.)).overflow_y_scrollbar().child(
                gpui_kit::base::SelectableText::new("selection-text", text.clone()),
            ))
            .footer(
                dialog::DialogFooter::new()
                    .child(
                        Button::new("selection-read")
                            .ghost()
                            .icon(gpui_kit::assets::IconName::Volume2)
                            .tooltip(tr("reader_read"))
                            .accessibility_label(tr("reader_read"))
                            .debug_selector(|| "system-selection-read".into())
                            .on_click(move |_, window, cx| {
                                crate::reader::toggle(id.clone(), &spoken, window, cx)
                            }),
                    )
                    .child(
                        Button::new("selection-insert")
                            .label(tr("message_quote"))
                            .disabled(chat.is_none())
                            .debug_selector(|| "system-selection-insert".into())
                            .on_click(move |_, window, cx| {
                                if let Some(chat) = &chat {
                                    if chat.insert(&quote, window, cx) {
                                        window.close_dialog(cx);
                                    } else {
                                        crate::feedback::error(
                                            "",
                                            &tr("selection_unavailable"),
                                            window,
                                            cx,
                                        );
                                    }
                                }
                            }),
                    ),
            )
    });
}
