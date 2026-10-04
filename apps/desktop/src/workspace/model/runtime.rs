use crate::tr;
use gpui_kit::SharedString;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub version: Option<SharedString>,
    pub system: Option<SharedString>,
    pub timezone: Option<SharedString>,
    pub uptime: Option<u64>,
    pub protocol_min: Option<u32>,
    pub protocol_max: Option<u32>,
    pub loading: bool,
    pub error: Option<SharedString>,
}

impl Snapshot {
    pub fn facts(&self) -> [(&'static str, SharedString); 5] {
        [
            ("host_version", optional(self.version.as_deref())),
            ("host_system_version", optional(self.system.as_deref())),
            ("host_timezone", optional(self.timezone.as_deref())),
            ("host_uptime", uptime(self.uptime)),
            (
                "host_protocol",
                protocol(self.protocol_min, self.protocol_max),
            ),
        ]
    }

    #[cfg(test)]
    pub fn sample(host: usize) -> Self {
        Self {
            version: Some("0.1.0-preview".into()),
            system: Some(if host == 0 { "macOS 15.0" } else { "Linux 6.8" }.into()),
            timezone: Some(if host == 0 { "Asia/Shanghai" } else { "UTC" }.into()),
            uptime: Some(if host == 0 { 93780 } else { 3660 }),
            protocol_min: Some(1),
            protocol_max: Some(if host == 0 { 1 } else { 3 }),
            ..Self::default()
        }
    }
}

fn optional(value: Option<&str>) -> SharedString {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_owned().into())
        .unwrap_or_else(|| tr("host_unknown"))
}

pub(crate) fn uptime(seconds: Option<u64>) -> SharedString {
    let Some(seconds) = seconds else {
        return tr("host_unknown");
    };
    let mut parts = Vec::new();
    for (key, value) in [
        ("host_uptime_days", seconds / 86400),
        ("host_uptime_hours", seconds / 3600 % 24),
        ("host_uptime_minutes", seconds / 60 % 60),
    ] {
        if value > 0 || (key == "host_uptime_minutes" && parts.is_empty()) {
            parts.push(rust_i18n::t!(key, count = value).to_string());
        }
    }
    parts.truncate(2);
    parts.join(" ").into()
}

fn protocol(minimum: Option<u32>, maximum: Option<u32>) -> SharedString {
    match (minimum, maximum) {
        (Some(min), Some(max)) if min == max => format!("v{min}").into(),
        (Some(min), Some(max)) if min < max => format!("v{min}-v{max}").into(),
        _ => tr("host_unknown"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_partial_facts() {
        assert!(
            Snapshot::default()
                .facts()
                .iter()
                .all(|(_, value)| *value == tr("host_unknown"))
        );
        assert_eq!(optional(Some(" \n\t")), tr("host_unknown"));
        assert_eq!(optional(Some("  version-preview \n")), "version-preview");
        assert_eq!(protocol(None, Some(3)), tr("host_unknown"));
        assert_eq!(protocol(Some(3), None), tr("host_unknown"));
        assert_eq!(protocol(Some(3), Some(1)), tr("host_unknown"));
        assert_eq!(protocol(Some(3), Some(3)), "v3");
        assert_eq!(protocol(Some(1), Some(3)), "v1-v3");
    }

    #[test]
    fn uptime_uses_two_nonzero_units() {
        rust_i18n::set_locale("zh-CN");
        let minutes = |n| rust_i18n::t!("host_uptime_minutes", count = n).to_string();
        let hours = |n| rust_i18n::t!("host_uptime_hours", count = n).to_string();
        let days = |n| rust_i18n::t!("host_uptime_days", count = n).to_string();
        assert_eq!(uptime(None), tr("host_unknown"));
        assert_eq!(uptime(Some(0)), minutes(0));
        assert_eq!(uptime(Some(59)), minutes(0));
        assert_eq!(uptime(Some(60)), minutes(1));
        assert_eq!(uptime(Some(3600)), hours(1));
        assert_eq!(uptime(Some(3660)), format!("{} {}", hours(1), minutes(1)));
        assert_eq!(uptime(Some(86460)), format!("{} {}", days(1), minutes(1)));
        assert_eq!(uptime(Some(93780)), format!("{} {}", days(1), hours(2)));
        assert!(!uptime(Some(u64::MAX)).is_empty());
    }
}
