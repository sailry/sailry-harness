//! Cross-platform native confirmation choices through RFD.
use crate::tr;
use gpui_kit::*;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

#[derive(Default)]
struct Active(Rc<RefCell<HashSet<WindowId>>>);
impl Global for Active {}

struct Pending(WindowId, Rc<RefCell<HashSet<WindowId>>>);
impl Drop for Pending {
    fn drop(&mut self) {
        self.1.borrow_mut().remove(&self.0);
    }
}

pub(crate) fn active(window: &Window, cx: &App) -> bool {
    cx.try_global::<Active>().is_some_and(|active| {
        active
            .0
            .borrow()
            .contains(&window.window_handle().window_id())
    })
}

pub(crate) fn ask(
    level: PromptLevel,
    title: &str,
    detail: &str,
    choices: &[SharedString],
    window: &mut Window,
    cx: &mut App,
) -> Task<Option<usize>> {
    if cx.try_global::<Active>().is_none() {
        cx.set_global(Active::default());
    }
    let id = window.window_handle().window_id();
    let active = cx.global::<Active>().0.clone();
    if !active.borrow_mut().insert(id) {
        return Task::ready(None);
    }
    let pending = Pending(id, active);
    let labels: Vec<_> = choices.iter().map(ToString::to_string).collect();
    let dialog = rfd::AsyncMessageDialog::new()
        .set_title(title)
        .set_description(detail)
        .set_level(match level {
            PromptLevel::Info => rfd::MessageLevel::Info,
            PromptLevel::Warning => rfd::MessageLevel::Warning,
            PromptLevel::Critical => rfd::MessageLevel::Error,
        })
        .set_buttons(buttons(&labels));
    #[cfg(not(test))]
    let response = dialog.set_parent(window).show();
    // Only the OS interaction is substituted in tests; button construction and
    // result mapping remain the same as the production path.
    #[cfg(test)]
    let response = {
        drop(dialog);
        tests::show(level, title, detail, choices, window, cx)
    };
    window.spawn(cx, async move |_| {
        let result = response.await;
        drop(pending);
        selection(&labels, result)
    })
}

fn buttons(labels: &[String]) -> rfd::MessageButtons {
    match labels {
        [ok] => rfd::MessageButtons::OkCustom(ok.clone()),
        [cancel, ok] => rfd::MessageButtons::OkCancelCustom(ok.clone(), cancel.clone()),
        [cancel, no, yes] => {
            rfd::MessageButtons::YesNoCancelCustom(yes.clone(), no.clone(), cancel.clone())
        }
        _ => unreachable!("native prompts support one to three choices"),
    }
}

fn selection(labels: &[String], result: rfd::MessageDialogResult) -> Option<usize> {
    use rfd::MessageDialogResult::*;
    match result {
        Custom(label) => labels.iter().position(|value| *value == label),
        Ok if labels.len() <= 2 => Some(labels.len() - 1),
        Yes if labels.len() == 3 => Some(2),
        No if labels.len() == 3 => Some(1),
        Cancel | Ok | Yes | No => None,
    }
}

pub(crate) fn confirm(
    title: &str,
    detail: &str,
    action: SharedString,
    window: &mut Window,
    cx: &mut App,
    accept: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let response = ask(
        PromptLevel::Warning,
        title,
        detail,
        &[tr("settings_cancel"), action],
        window,
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            if response.await == Some(1) {
                let _ = cx.update(accept);
            }
        })
        .detach();
}

#[cfg(test)]
pub(crate) mod tests;
