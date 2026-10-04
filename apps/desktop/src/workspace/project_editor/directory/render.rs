use super::*;
use gpui_kit::component::{spinner::Spinner, v_virtual_list};
use sailry_protocol::file_browser::LocationKind;
use std::rc::Rc;

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = (window.viewport_size().width - px(48.)).min(px(900.)) - px(48.);
        let height = (window.viewport_size().height - px(360.)).clamp(px(120.), px(520.));
        let rail = px(180.).min(width * 0.28);
        let grid = width - rail - px(12.);
        self.columns = ((f32::from(grid) - 20.) / 132.).floor().clamp(1., 8.) as usize;
        let sizes = Rc::new(vec![
            size(grid, px(110.));
            self.entries().len().div_ceil(self.columns)
        ]);
        let locations = self
            .listing
            .as_ref()
            .map(|listing| listing.locations.clone())
            .unwrap_or_default();
        v_flex()
            .id("directory-picker")
            .debug_selector(|| "directory-picker".into())
            .gap_3()
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(Self::dispatch))
            .on_action(cx.listener(
                |picker, _: &gpui_kit::component::dialog::Confirm, window, cx| {
                    if let Some(index) = picker.focused {
                        picker.activate(index, window, cx);
                    }
                    cx.stop_propagation();
                },
            ))
            .when(self.client.is_none(), |body| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("directory_preview")),
                )
            })
            .child(
                h_flex()
                    .gap_1()
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
                                .is_none_or(|listing| listing.parent.is_none()),
                            cx,
                            |picker, window, cx| picker.up(window, cx),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_1()
                            .debug_selector(|| "directory-address".into())
                            .child(
                                Input::new(&self.address)
                                    .disabled(self.busy)
                                    .aria_label(tr("directory_address")),
                            ),
                    )
                    .child(self.nav(
                        "directory_refresh",
                        IconName::RotateCw,
                        false,
                        cx,
                        |picker, window, cx| picker.refresh(window, cx),
                    )),
            )
            .child(
                h_flex()
                    .h(height)
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .relative()
                            .w(rail)
                            .h_full()
                            .flex_shrink_0()
                            .debug_selector(|| "directory-locations".into())
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted.opacity(0.35))
                            // Keep the frame outside the scrolled content so its border
                            // remains visible when locations overflow in a short window.
                            .child(
                                v_flex()
                                    .id("directory-locations-scroll")
                                    .size_full()
                                    .gap_1()
                                    .p_2()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.locations_scroll)
                                    .children(locations.into_iter().enumerate().map(
                                        |(index, location)| {
                                            let (label, icon) = match location.kind {
                                                LocationKind::Home => {
                                                    ("directory_home", IconName::Folder)
                                                }
                                                LocationKind::Desktop => {
                                                    ("directory_desktop", IconName::HardDrive)
                                                }
                                                LocationKind::Documents => {
                                                    ("directory_documents", IconName::FileText)
                                                }
                                                LocationKind::Downloads => {
                                                    ("directory_downloads", IconName::ArrowDown)
                                                }
                                                LocationKind::Volume => {
                                                    ("directory_volume", IconName::HardDrive)
                                                }
                                                LocationKind::Root => {
                                                    ("directory_root", IconName::Folder)
                                                }
                                            };
                                            let name = if location.name.is_empty() {
                                                tr(label)
                                            } else {
                                                location.name.into()
                                            };
                                            Button::new(("directory-location", index))
                                                .custom(crate::theme::subtle_button(cx))
                                                .justify_start()
                                                .w_full()
                                                .child(
                                                    h_flex()
                                                        .w_full()
                                                        .min_w_0()
                                                        .gap_2()
                                                        .child(
                                                            Icon::new(icon)
                                                                .size_4()
                                                                .flex_shrink_0(),
                                                        )
                                                        .child(
                                                            div()
                                                                .min_w_0()
                                                                .truncate()
                                                                .text_sm()
                                                                .child(name.clone()),
                                                        ),
                                                )
                                                .accessibility_label(name)
                                                .disabled(self.busy)
                                                .selected(self.listing.as_ref().is_some_and(
                                                    |listing| {
                                                        listing.directory.path == location.path
                                                    },
                                                ))
                                                .debug_selector(move || label.into())
                                                .on_click(cx.listener(
                                                    move |picker, _, window, cx| {
                                                        picker.navigate(
                                                            Some(location.path.clone()),
                                                            Visit::Push,
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                ))
                                        },
                                    )),
                            )
                            .vertical_scrollbar(&self.locations_scroll),
                    )
                    .child(
                        div()
                            .id("directory-grid")
                            .track_focus(&self.focus)
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .p_2()
                            .debug_selector(|| "directory-list".into())
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|picker, event, window, cx| {
                                    picker.menu(None, event, window, cx)
                                }),
                            )
                            .when(self.busy, |body| {
                                body.child(
                                    v_flex()
                                        .size_full()
                                        .justify_center()
                                        .items_center()
                                        .child(Spinner::new()),
                                )
                            })
                            .when(!self.busy && self.entries().is_empty(), |body| {
                                body.child(
                                    div()
                                        .size_full()
                                        .debug_selector(|| "directory-empty".into())
                                        .child(crate::empty_state::panel(
                                            IconName::FolderOpen,
                                            "files_directory_empty",
                                            cx,
                                        )),
                                )
                            })
                            .when(!self.busy && !self.entries().is_empty(), |body| {
                                body.child(
                                    v_virtual_list(
                                        cx.entity(),
                                        "directory-tiles",
                                        sizes,
                                        |picker, range, _, cx| {
                                            range
                                                .map(|row| {
                                                    h_flex()
                                                        .w_full()
                                                        .h(px(110.))
                                                        .gap(px(6.))
                                                        .items_start()
                                                        .children(
                                                            (row * picker.columns
                                                                ..((row + 1) * picker.columns)
                                                                    .min(picker.entries().len()))
                                                                .map(|index| {
                                                                    picker.tile(index, cx)
                                                                }),
                                                        )
                                                })
                                                .collect()
                                        },
                                    )
                                    .size_full()
                                    .track_scroll(&self.scroll),
                                )
                            }),
                    ),
            )
            .when(self.error.is_some(), |body| {
                body.child(
                    div()
                        .id("directory-error")
                        .debug_selector(|| "directory-error".into()),
                )
            })
    }
}

impl Picker {
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
            .ghost()
            .icon(icon)
            .disabled(self.busy || disabled)
            .tooltip(tr(label))
            .accessibility_label(tr(label))
            .debug_selector(move || id.into())
            .on_click(cx.listener(move |picker, _, window, cx| action(picker, window, cx)))
    }

    fn tile(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entry = &self.entries()[index];
        div()
            .id(("directory-tile", index))
            .w(relative(1. / self.columns as f32))
            .min_w_0()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |picker, event, window, cx| {
                    picker.menu(Some(index), event, window, cx)
                }),
            )
            .child(
                Button::new(("directory-entry", index))
                    .custom(crate::theme::subtle_button(cx))
                    .w_full()
                    .h(px(104.))
                    .p_2()
                    .selected(self.selected.contains(&index))
                    .disabled(!matches!(
                        entry.kind,
                        EntryKind::Directory | EntryKind::File
                    ))
                    .accessibility_label(entry.name.clone())
                    .debug_selector(move || format!("directory-row-{index}"))
                    .child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(
                                Icon::new(if entry.kind == EntryKind::Directory {
                                    IconName::Folder
                                } else {
                                    IconName::File
                                })
                                .size(px(34.))
                                .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_sm()
                                    .text_center()
                                    .whitespace_normal()
                                    .line_clamp(2)
                                    .child(entry.name.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |picker, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        let modifiers = window.modifiers();
                        picker.choose(index, modifiers.platform || modifiers.control, window, cx);
                        if event.click_count() == 2 {
                            picker.activate(index, window, cx);
                        }
                    })),
            )
    }
}
