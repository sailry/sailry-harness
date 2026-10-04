use crate::tr;
use gpui_kit::SharedString;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Disk {
    pub mount: SharedString,
    pub file_system: SharedString,
    pub total: u64,
    pub available: u64,
    pub read: u64,
    pub written: u64,
}

#[derive(Clone)]
pub(crate) struct Process {
    pub name: SharedString,
    pub cpu: f64,
    pub memory: u64,
}

pub(crate) struct Snapshot {
    pub cpu: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub received: u64,
    pub transmitted: u64,
    pub interval: Duration,
    pub disks: Vec<Disk>,
    pub processes: Vec<Process>,
}

impl Snapshot {
    pub fn sample(host: usize) -> Self {
        let gib = 1024 * 1024 * 1024;
        Self {
            cpu: if host == 0 { 12.5 } else { 24.0 },
            memory_used: if host == 0 { 8 * gib } else { 3 * gib },
            memory_total: if host == 0 { 32 * gib } else { 8 * gib },
            received: 512 * 1024,
            transmitted: 128 * 1024,
            interval: Duration::from_secs(2),
            disks: vec![Disk {
                mount: if host == 0 {
                    "/preview"
                } else {
                    "/preview/remote-volume"
                }
                .into(),
                file_system: if host == 0 { "APFS" } else { "ext4" }.into(),
                total: 256 * gib,
                available: 160 * gib,
                read: 8 * 1024 * 1024,
                written: 2 * 1024 * 1024,
            }],
            processes: vec![
                Process {
                    name: tr("metrics_worker"),
                    cpu: 8.0,
                    memory: 256 * 1024 * 1024,
                },
                Process {
                    name: tr("metrics_indexer"),
                    cpu: 4.5,
                    memory: 128 * 1024 * 1024,
                },
            ],
        }
    }
}

pub(super) fn bytes(value: f64) -> String {
    if !value.is_finite() || value < 0. {
        return tr("host_unknown").to_string();
    }
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = value;
    let mut unit = 0;
    while value >= 1024. && unit < units.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

pub(super) fn percent(value: f64) -> String {
    if value.is_finite() && value >= 0. {
        format!("{value:.1}%")
    } else {
        tr("host_unknown").to_string()
    }
}

pub(super) fn rate(value: u64, interval: Duration) -> String {
    if interval.is_zero() {
        return tr("host_unknown").to_string();
    }
    rust_i18n::t!(
        "metrics_rate",
        value = bytes(value as f64 / interval.as_secs_f64())
    )
    .to_string()
}

pub(super) fn usage(used: u64, total: u64) -> Option<f32> {
    (total > 0).then(|| ((used as f64 / total as f64) * 100.).clamp(0., 100.) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_rates_and_invalid_values() {
        rust_i18n::set_locale("zh-CN");
        assert_eq!(bytes(1024.), "1.0 KiB");
        assert_eq!(bytes(u64::MAX as f64), "16.0 EiB");
        assert_eq!(rate(2048, Duration::from_secs(2)), "1.0 KiB/s");
        assert_eq!(rate(1, Duration::ZERO), tr("host_unknown"));
        assert_eq!(percent(f64::NAN), tr("host_unknown"));
        assert_eq!(percent(-1.), tr("host_unknown"));
        assert_eq!(percent(250.), "250.0%");
        assert_eq!(bytes(f64::INFINITY), tr("host_unknown"));
        assert_eq!(usage(10, 0), None);
        assert_eq!(usage(20, 10), Some(100.));
        assert_eq!(usage(1, 4), Some(25.));
    }
}
