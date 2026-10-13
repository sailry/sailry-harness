use super::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

impl Picker {
    pub(super) fn cancel_children(&mut self) {
        if let Some(stop) = self.children_stop.take() {
            stop.cancel();
        }
    }

    pub(super) fn children(
        &mut self,
        path: String,
        generation: u64,
        menu: Entity<PopupMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed || self.busy || self.generation != generation || !(self.valid)(cx) {
            return;
        }
        self.cancel_children();
        let stop = sailry_link::CancellationToken::new();
        self.children_stop = Some(stop.clone());
        let Some(client) = self.client.clone() else {
            match self.catalog.list(&path) {
                Ok(listing) => self.fill_children(listing, menu, window, cx),
                Err(key) => self.children_failed(key, menu, window, cx),
            }
            return;
        };
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let job = runtime.spawn(navigation::read(client, Some(path), stop.clone()));
        let menu = menu.downgrade();
        cx.spawn_in(window, async move |picker, cx| {
            let result = job.await;
            let _ = picker.update_in(cx, |picker, window, cx| {
                if picker.closed
                    || picker.generation != generation
                    || stop.is_cancelled()
                    || !(picker.valid)(cx)
                {
                    return;
                }
                let Some(menu) = menu.upgrade() else {
                    return;
                };
                match result {
                    Ok(Ok(listing)) => picker.fill_children(listing, menu, window, cx),
                    _ => picker.children_failed("files_directory_failed", menu, window, cx),
                }
            });
        })
        .detach();
    }

    fn fill_children(
        &self,
        listing: Listing,
        menu: Entity<PopupMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let entries: Vec<_> = listing
            .directory
            .entries
            .into_iter()
            .filter(|entry| {
                entry.kind == EntryKind::Directory && (self.hidden || !entry.name.starts_with('.'))
            })
            .collect();
        menu.update(cx, |menu, cx| {
            menu.rebuild(window, cx, |menu, _, _| {
                if entries.is_empty() {
                    return menu.label(tr("directory_no_matches"));
                }
                entries.into_iter().fold(menu, |menu, entry| {
                    let path = format!(
                        "{}{}{}",
                        listing.directory.path.trim_end_matches(listing.separator),
                        listing.separator,
                        entry.name
                    );
                    let owner = owner.clone();
                    menu.item(
                        PopupMenuItem::new(entry.name)
                            .icon(IconName::Folder)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |picker, cx| {
                                    picker.navigate(Some(path.clone()), Visit::Push, window, cx)
                                });
                            }),
                    )
                })
            });
        });
    }

    fn children_failed(
        &self,
        key: &'static str,
        menu: Entity<PopupMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::feedback::toast(window, tr(key), Notification::error(tr(key)), cx);
        menu.update(cx, |_, cx| cx.emit(DismissEvent));
    }
}
