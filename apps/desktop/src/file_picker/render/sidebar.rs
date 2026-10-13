use super::*;
use gpui_kit::component::scroll::ScrollableElement;
use sailry_protocol::file_browser::LocationKind;
impl Picker {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let locations = self
            .listing
            .as_ref()
            .map(|value| value.locations.clone())
            .unwrap_or_default();
        let mut groups = Vec::new();
        for devices in [false, true] {
            groups.push(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(if devices {
                        "directory_devices"
                    } else {
                        "directory_locations"
                    }))
                    .into_any_element(),
            );
            for (index, location) in locations.iter().enumerate().filter(|(_, location)| {
                matches!(location.kind, LocationKind::Volume | LocationKind::Root) == devices
            }) {
                let (key, icon) = match location.kind {
                    LocationKind::Home => ("directory_home", gpui_kit::assets::IconName::House),
                    LocationKind::Desktop => {
                        ("directory_desktop", gpui_kit::assets::IconName::Monitor)
                    }
                    LocationKind::Documents => {
                        ("directory_documents", gpui_kit::assets::IconName::FileText)
                    }
                    LocationKind::Downloads => {
                        ("directory_downloads", gpui_kit::assets::IconName::Download)
                    }
                    LocationKind::Volume => {
                        ("directory_volume", gpui_kit::assets::IconName::HardDrive)
                    }
                    LocationKind::Root => ("directory_root", gpui_kit::assets::IconName::HardDrive),
                };
                let name = if location.name.is_empty() {
                    tr(key)
                } else {
                    location.name.clone().into()
                };
                let path = location.path.clone();
                groups.push(
                    Button::new(("directory-location", index))
                        .small()
                        .custom(crate::theme::subtle_button(cx))
                        .w_full()
                        .h(px(28.))
                        .justify_start()
                        .accessibility_label(name.clone())
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .justify_start()
                                .gap_2()
                                .child(Icon::new(icon).size_3p5().flex_shrink_0())
                                .child(
                                    div()
                                        .debug_selector(move || {
                                            format!("directory-location-label-{index}")
                                        })
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(cx.theme().typography_tokens().sm.size)
                                        .truncate()
                                        .child(name),
                                ),
                        )
                        .disabled(self.busy)
                        .selected(
                            self.listing
                                .as_ref()
                                .is_some_and(|value| value.directory.path == path),
                        )
                        .debug_selector(move || key.into())
                        .on_click(cx.listener(move |picker, _, window, cx| {
                            picker.navigate(Some(path.clone()), Visit::Push, window, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        div()
            .w(px(188.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.35))
            .debug_selector(|| "directory-locations".into())
            .child(
                v_flex()
                    .id("directory-locations-scroll")
                    .size_full()
                    .p_2()
                    .gap_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.locations_scroll)
                    .children(groups),
            )
            .vertical_scrollbar(&self.locations_scroll)
    }
}
