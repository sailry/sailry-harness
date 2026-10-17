//! Read and write throughput of the selected partition.
use super::resources::{Tones, half, mono, panel, rate, series, stat, title, transfer_chart};
use super::*;
use sailry_protocol::host::metrics::IoRate;

impl Monitor {
    pub(super) fn disk_card(&self, cx: &App) -> impl IntoElement {
        let tones = Tones::of(cx);
        let io = self.view.sample.as_deref().and_then(|sample| {
            sample
                .disks
                .iter()
                .find(|disk| Some(&disk.mount) == self.disk.as_ref())
                .and_then(|disk| disk.io.as_ref())
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
                            io.written_bytes_per_sec as f64,
                            io.read_bytes_per_sec as f64,
                        )
                    }),
            )
        }));
        let value =
            |rate_of: fn(&IoRate) -> u64| io.map_or_else(|| "—".into(), |io| rate(rate_of(io)));
        half(panel("host-disk", cx))
            .child(
                h_flex()
                    .gap_2()
                    .min_w_0()
                    .debug_selector(|| "host-disk".into())
                    .child(title("metrics_disks"))
                    .when_some(self.disk.clone(), |row, mount| {
                        row.child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .min_w_0()
                                .max_w(px(180.))
                                .child(mono(div().truncate(), cx).child(mount)),
                        )
                    }),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_x_5()
                    .gap_y_3()
                    .debug_selector(|| "host-disk-detail".into())
                    .child(stat(
                        "host_disk_write",
                        value(|io| io.written_bytes_per_sec),
                        None,
                        tones.write,
                        cx,
                    ))
                    .child(stat(
                        "host_disk_read",
                        value(|io| io.read_bytes_per_sec),
                        None,
                        tones.read,
                        cx,
                    )),
            )
            .child(transfer_chart(
                "host-disk-chart",
                points,
                (tones.write, tr("host_disk_write")),
                (tones.read, tr("host_disk_read")),
                cx,
            ))
    }
}
