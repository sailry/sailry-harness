//! Interval rates from host interface counters, without loopback traffic.
use sailry_protocol::host::metrics::NetworkRate;
use std::{collections::BTreeMap, time::Duration};
use sysinfo::Networks;

type Counters = BTreeMap<String, (u64, u64)>;

pub(super) fn sample(networks: &mut Networks, interval: Option<Duration>) -> Option<NetworkRate> {
    let previous = counters(networks);
    networks.refresh(true);
    rates(&previous, &counters(networks), interval)
}

fn counters(networks: &Networks) -> Counters {
    networks
        .iter()
        .filter(|(_, data)| {
            data.ip_networks()
                .iter()
                .any(|network| !network.addr.is_loopback())
        })
        .map(|(name, data)| {
            (
                name.clone(),
                (data.total_received(), data.total_transmitted()),
            )
        })
        .collect()
}

fn rates(
    previous: &Counters,
    current: &Counters,
    interval: Option<Duration>,
) -> Option<NetworkRate> {
    let interval =
        interval.filter(|interval| !interval.is_zero() && *interval < Duration::from_secs(5))?;
    let mut totals = None;
    for (name, (received, transmitted)) in current {
        let Some((old_received, old_transmitted)) = previous.get(name) else {
            continue;
        };
        let (Some(received), Some(transmitted)) = (
            received.checked_sub(*old_received),
            transmitted.checked_sub(*old_transmitted),
        ) else {
            continue;
        };
        let (incoming, outgoing) = totals.get_or_insert((0u64, 0u64));
        *incoming = incoming.saturating_add(received);
        *outgoing = outgoing.saturating_add(transmitted);
    }
    totals.map(|(received, transmitted)| NetworkRate {
        received_bytes_per_sec: (received as f64 / interval.as_secs_f64()) as u64,
        transmitted_bytes_per_sec: (transmitted as f64 / interval.as_secs_f64()) as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(entries: &[(&str, u64, u64)]) -> Counters {
        entries
            .iter()
            .map(|(name, received, transmitted)| (name.to_string(), (*received, *transmitted)))
            .collect()
    }

    #[test]
    fn measures_elapsed_rates() {
        let before = counters(&[("ethernet", 1000, 2000), ("wifi", 500, 700)]);
        let after = counters(&[("ethernet", 5000, 8000), ("wifi", 1500, 2700)]);
        assert_eq!(
            rates(&before, &after, Some(Duration::from_secs(2))),
            Some(NetworkRate {
                received_bytes_per_sec: 2500,
                transmitted_bytes_per_sec: 4000,
            })
        );
        assert_eq!(
            rates(&after, &after, Some(Duration::from_secs(1))),
            Some(NetworkRate {
                received_bytes_per_sec: 0,
                transmitted_bytes_per_sec: 0,
            })
        );
    }

    #[test]
    fn skips_discontinuous_counters() {
        let before = counters(&[
            ("stable", 100, 200),
            ("removed", 1000, 1000),
            ("reset", 500, 500),
        ]);
        let after = counters(&[
            ("stable", 110, 220),
            ("new", 999999, 999999),
            ("reset", 1, 1),
        ]);
        assert_eq!(
            rates(&before, &after, Some(Duration::from_secs(1))),
            Some(NetworkRate {
                received_bytes_per_sec: 10,
                transmitted_bytes_per_sec: 20,
            })
        );
    }

    #[test]
    fn omits_unavailable_samples() {
        let values = counters(&[("wifi", 500, 700)]);
        for interval in [None, Some(Duration::ZERO), Some(Duration::from_secs(5))] {
            assert_eq!(rates(&values, &values, interval), None);
        }
        assert_eq!(
            rates(&Counters::new(), &values, Some(Duration::from_secs(1))),
            None
        );
    }
}
