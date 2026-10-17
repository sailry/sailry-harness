use super::*;

pub(super) fn crumbs(listing: &Listing) -> Vec<(String, String)> {
    let path = &listing.directory.path;
    let separator = listing.separator;
    let mut root = if separator == '\\' {
        path.split(separator).next().unwrap_or("").to_owned()
    } else {
        "/".into()
    };
    let mut label = root.clone();
    if separator == '\\' && !root.is_empty() {
        root.push(separator);
    }
    if let Some(location) = listing
        .locations
        .iter()
        .filter(|location| {
            matches!(
                location.kind,
                sailry_protocol::file_browser::LocationKind::Volume
                    | sailry_protocol::file_browser::LocationKind::Root
            )
        })
        .filter(|location| {
            path == &location.path
                || path.strip_prefix(&location.path).is_some_and(|suffix| {
                    suffix.starts_with(separator) || location.path.ends_with(separator)
                })
        })
        .max_by_key(|location| location.path.len())
    {
        root = location.path.clone();
        if !location.name.is_empty() {
            label = location.name.clone();
        }
    }
    // UNC roots are supplied by the Node, not parsed with the controller's OS.
    if separator == '\\' && root.is_empty() {
        let components: Vec<_> = path
            .split(separator)
            .filter(|value| !value.is_empty())
            .take(2)
            .collect();
        root = format!("\\\\{}", components.join("\\"));
        label = root.clone();
    }
    let mut result = vec![(label, root.clone())];
    let mut current = root.trim_end_matches(separator).to_owned();
    for part in path
        .strip_prefix(&root)
        .unwrap_or(path)
        .split(separator)
        .filter(|part| !part.is_empty())
    {
        current.push(separator);
        current.push_str(part);
        result.push((part.into(), current.clone()));
    }
    result
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    fn listing(path: &str, separator: char) -> Listing {
        Listing {
            separator,
            parent: None,
            locations: Vec::new(),
            directory: sailry_protocol::Directory {
                path: path.into(),
                entries: Vec::new(),
                truncated: false,
                unsupported_names: 0,
                revision: "fixture".into(),
                next: None,
            },
        }
    }
    #[test]
    fn drive_root_stays_absolute() {
        assert_eq!(
            crumbs(&listing("D:\\source\\目录", '\\')),
            vec![
                ("D:".into(), "D:\\".into()),
                ("source".into(), "D:\\source".into()),
                ("目录".into(), "D:\\source\\目录".into())
            ]
        );
    }
    #[test]
    fn unc_preserves_share_root() {
        assert_eq!(
            crumbs(&listing("\\\\node\\share\\folder", '\\')),
            vec![
                ("\\\\node\\share".into(), "\\\\node\\share".into()),
                ("folder".into(), "\\\\node\\share\\folder".into())
            ]
        );
    }
    #[test]
    fn volume_name_comes_from_node() {
        let mut listing = listing("/Volumes/Work/source", '/');
        listing
            .locations
            .push(sailry_protocol::file_browser::Location {
                kind: sailry_protocol::file_browser::LocationKind::Volume,
                name: "Work".into(),
                path: "/Volumes/Work".into(),
            });
        assert_eq!(
            crumbs(&listing),
            vec![
                ("Work".into(), "/Volumes/Work".into()),
                ("source".into(), "/Volumes/Work/source".into())
            ]
        );
    }
}
