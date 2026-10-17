//! Partition tiles; the selected tile drives the disk throughput chart.
use super::resources::{Tones, bytes, mono, panel, rate, title};
use super::*;
use gpui_kit::component::button::ButtonVariants;

impl Monitor {
    pub(super) fn storage_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tones = Tones::of(cx);
        let disks = self
            .view
            .sample
            .as_deref()
            .map(|sample| sample.disks.as_slice())
            .unwrap_or_default();
        panel("host-storage", cx)
            .child(
                h_flex()
                    .items_baseline()
                    .gap_2()
                    .child(title("host_storage"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                rust_i18n::t!("host_partition_count", count = disks.len())
                                    .to_string(),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_3()
                    .gap_y_1()
                    .min_w_0()
                    .debug_selector(|| "host-partitions".into())
                    .children(disks.iter().enumerate().map(|(index, disk)| {
                        let owner = cx.entity();
                        let mount = disk.mount.clone();
                        let current = self.disk.as_ref() == Some(&mount);
                        let used = disk.total_bytes.saturating_sub(disk.available_bytes);
                        let share = if disk.total_bytes == 0 {
                            0.
                        } else {
                            used as f32 / disk.total_bytes as f32
                        };
                        let detail = [
                            disk.file_system.clone(),
                            format!("{} / {}", bytes(used), bytes(disk.total_bytes)),
                            format!("{:.1}%", share * 100.),
                        ]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                        let rates = [
                            (
                                "host_read_short",
                                disk.io.as_ref().map(|io| io.read_bytes_per_sec),
                            ),
                            (
                                "host_write_short",
                                disk.io.as_ref().map(|io| io.written_bytes_per_sec),
                            ),
                        ];
                        Button::new(("host-partition", index))
                            .ghost()
                            .selected(current)
                            .toggled(current)
                            .flex_1()
                            .flex_basis(px(400.))
                            .flex_shrink()
                            .min_w_0()
                            .h_auto()
                            .px_3p5()
                            .py_3()
                            .rounded(cx.theme().radius_lg)
                            .accessibility_label(mount.clone())
                            .debug_selector(move || format!("host-partition-{index}"))
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_sm()
                                                    .when(current, |label| label.font_semibold())
                                                    .debug_selector(move || {
                                                        format!("host-partition-label-{index}")
                                                    })
                                                    .child(mount.clone()),
                                            )
                                            .child(mono(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(detail),
                                                cx,
                                            )),
                                    )
                                    .child(mono(
                                        v_flex().flex_shrink_0().gap_1().children(rates.map(
                                            |(label, value)| {
                                                h_flex()
                                                    .items_baseline()
                                                    .justify_end()
                                                    .gap_2p5()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(tr(label)),
                                                    )
                                                    .child(
                                                        div()
                                                            .min_w(px(84.))
                                                            .text_right()
                                                            .text_sm()
                                                            .child(
                                                                value.map_or_else(
                                                                    || "—".into(),
                                                                    rate,
                                                                ),
                                                            ),
                                                    )
                                            },
                                        )),
                                        cx,
                                    ))
                                    .child(
                                        v_flex()
                                            .w(px(14.))
                                            .h(px(44.))
                                            .flex_shrink_0()
                                            .justify_end()
                                            .overflow_hidden()
                                            .rounded(px(4.))
                                            .bg(tones.track)
                                            .child(
                                                div()
                                                    .w_full()
                                                    .h(relative(share))
                                                    .bg(tones.capacity(share * 100.)),
                                            ),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |this, cx| {
                                    this.disk = Some(mount.clone());
                                    cx.notify();
                                })
                            })
                    })),
            )
    }
}
