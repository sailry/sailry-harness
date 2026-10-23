use super::*;
use gpui_kit::component::{
    button::{Toggle, ToggleGroup, ToggleVariants},
    menu::DropdownMenu,
    scroll::ScrollableElement as _,
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
            .flex_shrink_0()
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
        let breadcrumb = h_flex()
            .id("directory-breadcrumbs")
            .flex_1()
            .min_w_0()
            .gap_1()
            .overflow_x_scroll()
            .track_scroll(&self.address_scroll)
            .children(
                breadcrumbs
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, path))| {
                        let owner = cx.entity().downgrade();
                        let dismiss = owner.clone();
                        let generation = self.generation;
                        h_flex()
                            .flex_shrink_0()
                            .min_w_0()
                            .gap_1()
                            .when(index > 0, |segment| {
                                segment.child(Icon::new(IconName::ChevronRight).size_3p5())
                            })
                            .child(
                                Button::new(("directory-crumb", index))
                                    .small()
                                    .text()
                                    .max_w(px(160.))
                                    .overflow_hidden()
                                    .accessibility_label(path.clone())
                                    .tooltip(path.clone())
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
                                            let _ = dismiss
                                                .update(cx, |picker, _| picker.cancel_children());
                                        }
                                    }),
                            )
                    }),
            )
            .horizontal_scrollbar(&self.address_scroll);
        let address = div()
            .id("directory-address")
            .debug_selector(|| "directory-address".into())
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .when(self.editing_address, |body| {
                body.on_mouse_down_out(cx.listener(|picker, _, window, cx| {
                    picker.editing_address = false;
                    if picker.address.read(cx).focus_handle(cx).is_focused(window) {
                        picker.focus.focus(window, cx);
                    }
                    cx.notify();
                }))
                .child(
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
                        .w_full()
                        .gap_2()
                        .min_w_0()
                        .overflow_hidden()
                        .child(Icon::new(IconName::HardDrive).size_4().flex_shrink_0())
                        .child(breadcrumb),
                )
            });
        let views = ToggleGroup::new("directory-view")
            .flex_shrink_0()
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
            .w_full()
            .min_w_0()
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
                    .flex_shrink_0()
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
            .child(self.nav(
                "directory_go_to",
                gpui_kit::assets::IconName::FolderInput,
                self.listing.is_none(),
                cx,
                |picker, window, cx| picker.go_to(window, cx),
            ))
    }
    fn nav(
        &self,
        id: &'static str,
        icon: impl Into<Icon>,
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
            .flex_shrink_0()
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
