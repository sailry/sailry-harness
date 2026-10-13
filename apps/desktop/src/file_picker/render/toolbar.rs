use super::*;
use gpui_kit::component::{
    button::{Toggle, ToggleGroup, ToggleVariants},
    menu::DropdownMenu,
};
impl Picker {
    pub(super) fn toolbar(
        &self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let breadcrumbs = self.listing.as_ref().map(crumbs).unwrap_or_default();
        let last = breadcrumbs.len().saturating_sub(1);
        let navigation = h_flex()
            .gap_0()
            .child(self.nav(
                "directory_back",
                IconName::ArrowLeft,
                self.cursor == 0,
                cx,
                |picker, window, cx| picker.step(false, window, cx),
            ))
            .child(self.nav(
                "directory_forward",
                IconName::ArrowRight,
                self.cursor + 1 >= self.history.len(),
                cx,
                |picker, window, cx| picker.step(true, window, cx),
            ))
            .child(
                self.nav(
                    "directory-up",
                    IconName::ArrowUp,
                    self.listing
                        .as_ref()
                        .is_none_or(|value| value.parent.is_none()),
                    cx,
                    |picker, window, cx| picker.up(window, cx),
                ),
            );
        // Kit BreadcrumbItem cannot host a selectable dropdown trigger. These
        // supported Kit buttons remain navigation controls, not a copied widget.
        let breadcrumb =
            h_flex()
                .min_w_0()
                .gap_1()
                .children(
                    breadcrumbs
                        .into_iter()
                        .enumerate()
                        .map(|(index, (label, path))| {
                            let owner = cx.entity().downgrade();
                            let dismiss = owner.clone();
                            let generation = self.generation;
                            h_flex()
                                .min_w_0()
                                .gap_1()
                                .when(index > 0, |segment| {
                                    segment.child(Icon::new(IconName::ChevronRight).size_3p5())
                                })
                                .child(
                                    Button::new(("directory-crumb", index))
                                        .small()
                                        .text()
                                        .label(label)
                                        .disabled(self.busy)
                                        .on_click(|_, _, cx| cx.stop_propagation())
                                        .when(index == last, |button| {
                                            button.font_weight(FontWeight::SEMIBOLD)
                                        })
                                        .debug_selector(move || format!("directory-crumb-{index}"))
                                        .dropdown_menu(move |menu, window, cx| {
                                            let owner = owner.clone();
                                            let path = path.clone();
                                            let popup = cx.entity().downgrade();
                                            cx.defer_in(window, move |_, window, cx| {
                                                if let Some(popup) = popup.upgrade() {
                                                    let _ = owner.update(cx, |picker, cx| {
                                                        picker.children(
                                                            path, generation, popup, window, cx,
                                                        )
                                                    });
                                                }
                                            });
                                            menu.scrollable(true)
                                                .max_h(px(320.))
                                                .label(tr("plugins_loading"))
                                        })
                                        .on_open_change(move |open, _, cx| {
                                            if !*open {
                                                let _ = dismiss.update(cx, |picker, _| {
                                                    picker.cancel_children()
                                                });
                                            }
                                        }),
                                )
                        }),
                );
        let address = div()
            .id("directory-address")
            .debug_selector(|| "directory-address".into())
            .flex_1()
            .min_w_0()
            .when(self.editing_address, |body| {
                body.child(
                    Input::new(&self.address)
                        .small()
                        .disabled(self.busy)
                        .aria_label(tr("directory_address")),
                )
            })
            .when(!self.editing_address, |body| {
                body.on_click(cx.listener(|picker, _, window, cx| {
                    picker.editing_address = true;
                    picker
                        .address
                        .update(cx, |input, cx| input.focus(window, cx));
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .overflow_hidden()
                        .child(Icon::new(IconName::HardDrive).size_4().flex_shrink_0())
                        .child(breadcrumb),
                )
            });
        let views = ToggleGroup::new("directory-view")
            .small()
            .outline()
            .segmented()
            .child(
                Toggle::new("directory-view-list")
                    .icon(gpui_kit::assets::IconName::List)
                    .checked(!self.grid)
                    .tooltip(tr("directory_list")),
            )
            .child(
                Toggle::new("directory-view-grid")
                    .icon(gpui_kit::assets::IconName::LayoutGrid)
                    .checked(self.grid)
                    .tooltip(tr("directory_grid")),
            )
            .on_click(cx.listener(|picker, values: &Vec<bool>, _, cx| {
                let list = values.first().copied().unwrap_or(false);
                let grid = values.get(1).copied().unwrap_or(false);
                picker.grid = match (list, grid) {
                    (true, true) => !picker.grid,
                    (false, true) => true,
                    (true, false) => false,
                    _ => picker.grid,
                };
                picker.preference(cx);
                cx.notify();
            }));
        h_flex()
            .gap_2()
            .child(navigation)
            .child(address)
            .child(
                div().w(px(168.)).flex_shrink_0().child(
                    Input::new(&self.filter)
                        .small()
                        .prefix(Icon::new(IconName::Search).size_3p5())
                        .cleanable(true)
                        .aria_label(tr("directory_filter")),
                ),
            )
            .child(views)
            .child(
                Toggle::new("directory-hidden")
                    .small()
                    .icon(if self.hidden {
                        IconName::Eye
                    } else {
                        IconName::EyeOff
                    })
                    .checked(self.hidden)
                    .tooltip(tr("directory_hidden"))
                    .on_click(cx.listener(|picker, checked, _, cx| {
                        picker.hidden = *checked;
                        picker.preference(cx);
                        picker.rebuild(cx);
                        cx.notify();
                    })),
            )
            .child(self.nav(
                "directory_refresh",
                IconName::RotateCw,
                false,
                cx,
                |picker, window, cx| picker.refresh(window, cx),
            ))
    }
    fn nav(
        &self,
        id: &'static str,
        icon: IconName,
        disabled: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Button {
        let label = if id == "directory-up" {
            "directory_up"
        } else {
            id
        };
        Button::new(id)
            .small()
            .ghost()
            .icon(icon)
            .disabled(self.busy || disabled)
            .tooltip(tr(label))
            .accessibility_label(tr(label))
            .debug_selector(move || id.into())
            .on_click(cx.listener(move |picker, _, window, cx| action(picker, window, cx)))
    }
}

fn crumbs(listing: &Listing) -> Vec<(String, String)> {
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
