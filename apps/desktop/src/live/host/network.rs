//! Network throughput with cumulative interface totals.
use super::resources::{Tones, bytes, half, panel, rate, series, stat, title, transfer_chart};
use super::*;
use sailry_protocol::host::metrics::NetworkRate;

impl Monitor {
    pub(super) fn network_card(&self, cx: &App) -> impl IntoElement {
        let tones = Tones::of(cx);
        let traffic = self
            .view
            .sample
            .as_ref()
            .and_then(|sample| sample.network.as_ref());
        let points = series(self.view.history.iter().map(|point| {
            (
                point.sampled_at_ms,
                point.network.as_ref().map(|traffic| {
                    (
                        traffic.received_bytes_per_sec as f64,
                        traffic.transmitted_bytes_per_sec as f64,
                    )
                }),
            )
        }));
        let value = |rate_of: fn(&NetworkRate) -> u64| {
            traffic.map_or_else(|| "—".into(), |traffic| rate(rate_of(traffic)))
        };
        let total =
            |total_of: fn(&NetworkRate) -> u64| traffic.map(|traffic| bytes(total_of(traffic)));
        half(panel("host-network", cx))
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .debug_selector(|| "host-network".into())
                    .child(title("metrics_network"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("host_network_total")),
                    ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_x_5()
                    .gap_y_3()
                    .debug_selector(|| "host-network-detail".into())
                    .child(stat(
                        "metrics_download",
                        value(|traffic| traffic.received_bytes_per_sec),
                        total(|traffic| traffic.received_bytes),
                        tones.download,
                        cx,
                    ))
                    .child(stat(
                        "metrics_upload",
                        value(|traffic| traffic.transmitted_bytes_per_sec),
                        total(|traffic| traffic.transmitted_bytes),
                        tones.upload,
                        cx,
                    )),
            )
            .child(transfer_chart(
                "host-network-chart",
                points,
                (tones.download, tr("metrics_download")),
                (tones.upload, tr("metrics_upload")),
                cx,
            ))
    }
}
