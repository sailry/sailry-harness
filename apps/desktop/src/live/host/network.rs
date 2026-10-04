use super::resources::{card, chart, rate, series};
use super::*;

impl Monitor {
    pub(super) fn network_card(&self, cx: &App) -> impl IntoElement {
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
                        traffic.received_bytes_per_sec as f64 / 1_048_576.,
                        traffic.transmitted_bytes_per_sec as f64 / 1_048_576.,
                    )
                }),
            )
        }));
        card(
            "host-network",
            "metrics_network",
            traffic
                .map(|traffic| {
                    format!(
                        "↑ {}   ↓ {}",
                        rate(traffic.transmitted_bytes_per_sec),
                        rate(traffic.received_bytes_per_sec),
                    )
                })
                .unwrap_or_else(|| "—".into()),
            chart(
                "host-network-chart",
                points,
                cx.theme().chart_2,
                "host_network_received",
                Some((cx.theme().chart_3, "host_network_transmitted")),
                cx,
            ),
            div(),
            cx,
        )
    }
}
