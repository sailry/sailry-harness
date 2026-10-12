//! Card targets occupy the inspector without entering Browser or Agent inventories.
use super::*;
use crate::conversation::live::View as Chat;
use crate::plugins::cards::{Card, browser::Browser};
use sailry_protocol::plugin::card;

pub(crate) fn open(
    owner: WeakEntity<Card>,
    source: WeakEntity<Chat>,
    url: String,
    target: card::Target,
    redirect: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(Some(root)) = window.root::<Root>() else {
        return;
    };
    let Ok(shell) = root.read(cx).view().clone().downcast::<Shell>() else {
        return;
    };
    window.defer(cx, move |window, cx| {
        shell.update(cx, |shell, cx| {
            let Some(source_entity) = source.upgrade() else {
                return;
            };
            if shell.current_chat() != Some(&source_entity)
                || owner
                    .upgrade()
                    .is_none_or(|owner| !owner.read(cx).active(cx))
            {
                return;
            }
            let retry = (
                owner.clone(),
                source.clone(),
                url.clone(),
                target.clone(),
                redirect.clone(),
            );
            if shell.guard_file_navigation(window, cx, move |_, window, cx| {
                open(
                    retry.0.clone(),
                    retry.1.clone(),
                    retry.2.clone(),
                    retry.3.clone(),
                    retry.4.clone(),
                    window,
                    cx,
                )
            }) {
                return;
            }
            let prepared = Browser::new(owner, source, url, target, redirect, window, cx);
            // Store only a successfully constructed native view; a failed mount
            // cannot replace the user's current inspector or drafts.
            let Ok(browser) = prepared else {
                window
                    .push_notification(notification::Notification::error(tr("plugins_failed")), cx);
                return;
            };
            shell.side_resource = Some(SideResource::CardBrowser(browser));
            shell.layout.panel_open[0] = true;
            cx.notify();
        });
    });
}
