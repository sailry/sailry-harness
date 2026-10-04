//! One I/O chart with a complete partition inventory underneath.
use super::resources::{bytes, card, chart, rate, series};
use super::*;
use gpui_kit::component::{button::ButtonVariants, table::*};

impl Monitor {
    pub(super) fn disk_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sample = self.view.sample.as_deref();
        let selected = sample.and_then(|sample| {
            sample
                .disks
                .iter()
                .find(|disk| Some(&disk.mount) == self.disk.as_ref())
        });
        let points = series(self.view.history.iter().map(|point| {
            (
                point.sampled_at_ms,
                self.disk
                    .as_ref()
                    .and_then(|mount| point.disks.get(mount))
                    .and_then(|io| io.as_ref())
                    .map(|io| {
                        (
                            io.read_bytes_per_sec as f64 / 1_048_576.,
                            io.written_bytes_per_sec as f64 / 1_048_576.,
                        )
                    }),
            )
        }));
        let header = TableHeader::new().child(
            TableRow::new().children(
                ["host_partition", "host_disk_available", "host_disk_total"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, key)| {
                        TableHead::new()
                            .col_span(if index == 0 { 2 } else { 1 })
                            .min_w(px(if index == 0 { 120. } else { 80. }))
                            .when(index > 0, |cell| cell.text_right())
                            .child(
                                div()
                                    .text_sm()
                                    .debug_selector(move || {
                                        format!("host-partition-heading-{index}")
                                    })
                                    .child(tr(key)),
                            )
                    }),
            ),
        );
        let partitions = Table::new()
            .accessibility_label(tr("host_partitions"))
            .child(header)
            .child(
                TableBody::new().children(
                    sample
                        .into_iter()
                        .flat_map(|sample| &sample.disks)
                        .enumerate()
                        .map(|(index, disk)| {
                            let owner = cx.entity();
                            let mount = disk.mount.clone();
                            let current = self.disk.as_ref() == Some(&mount);
                            TableRow::new()
                                .child(
                                    TableCell::new().col_span(2).min_w(px(120.)).child(
                                        Button::new(("host-partition", index))
                                            .small()
                                            .ghost()
                                            .w_full()
                                            .min_w_0()
                                            .px_0()
                                            .accessibility_label(mount.clone())
                                            .child(
                                                h_flex().w_full().min_w_0().child(
                                                    div()
                                                        .min_w_0()
                                                        .truncate()
                                                        .text_sm()
                                                        .line_height(relative(1.))
                                                        .when(current, |label| {
                                                            label.font_semibold()
                                                        })
                                                        .debug_selector(move || {
                                                            format!("host-partition-label-{index}")
                                                        })
                                                        .child(mount.clone()),
                                                ),
                                            )
                                            .debug_selector(move || {
                                                format!("host-partition-{index}")
                                            })
                                            .on_click(move |_, _, cx| {
                                                owner.update(cx, |this, cx| {
                                                    this.disk = Some(mount.clone());
                                                    cx.notify();
                                                })
                                            }),
                                    ),
                                )
                                .child(
                                    TableCell::new()
                                        .min_w(px(80.))
                                        .text_right()
                                        .child(bytes(disk.available_bytes)),
                                )
                                .child(
                                    TableCell::new()
                                        .min_w(px(80.))
                                        .text_right()
                                        .child(bytes(disk.total_bytes)),
                                )
                        }),
                ),
            );
        card(
            "host-disk",
            "metrics_disks",
            selected
                .and_then(|disk| disk.io.as_ref())
                .map(|io| {
                    format!(
                        "↑ {}   ↓ {}",
                        rate(io.written_bytes_per_sec),
                        rate(io.read_bytes_per_sec)
                    )
                })
                .unwrap_or_else(|| "—".into()),
            chart(
                "host-disk-chart",
                points,
                cx.theme().chart_2,
                "host_disk_read",
                Some((cx.theme().chart_4, "host_disk_write")),
                cx,
            ),
            div()
                .id("host-partitions")
                .w_full()
                .min_w_0()
                .overflow_x_scroll()
                .child(partitions),
            cx,
        )
    }
}
