use super::Settings;
use system_configuration::{
    core_foundation::{
        array::CFArray,
        base::{CFType, TCFType},
        dictionary::CFDictionary,
        number::CFNumber,
        string::CFString,
    },
    dynamic_store::SCDynamicStoreBuilder,
};

fn endpoint(values: &CFDictionary<CFString, CFType>, scheme: &str) -> String {
    let number = |key: &str| {
        values
            .find(&CFString::new(key))
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i32())
    };
    if number(&format!("{scheme}Enable")) != Some(1) {
        return String::new();
    }
    let host = values
        .find(&CFString::new(&format!("{scheme}Proxy")))
        .and_then(|value| value.downcast::<CFString>())
        .map(|value| value.to_string());
    let port = number(&format!("{scheme}Port"))
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port != 0);
    match (host, port) {
        (Some(host), Some(port)) => format!(
            "http://{}:{port}",
            if host.parse::<std::net::Ipv6Addr>().is_ok() {
                format!("[{host}]")
            } else {
                host
            }
        ),
        _ => String::new(),
    }
}

pub(super) fn settings() -> Settings {
    SCDynamicStoreBuilder::new("Sailry web fetch")
        .build()
        .and_then(|store| store.get_proxies())
        .map(|values| parse(&values))
        .unwrap_or_default()
}

fn parse(values: &CFDictionary<CFString, CFType>) -> Settings {
    let bypass = values
        .find(&CFString::new("ExceptionsList"))
        .and_then(|value| value.downcast::<CFArray>())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    // SAFETY: System Configuration's ExceptionsList contains retained
                    // Core Foundation objects; downcast checks each object's type.
                    let value = unsafe { CFType::wrap_under_get_rule(*value) };
                    value
                        .downcast::<CFString>()
                        .map(|value| value.to_string().trim_start_matches("*.").to_owned())
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    Settings {
        http: endpoint(values, "HTTP"),
        https: endpoint(values, "HTTPS"),
        bypass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_enabled_endpoints_and_exclusions() {
        let values = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("HTTPEnable"),
                CFNumber::from(1i32).into_CFType(),
            ),
            (
                CFString::new("HTTPProxy"),
                CFString::new("::1").into_CFType(),
            ),
            (
                CFString::new("HTTPPort"),
                CFNumber::from(7890i32).into_CFType(),
            ),
            (
                CFString::new("HTTPSEnable"),
                CFNumber::from(0i32).into_CFType(),
            ),
            (
                CFString::new("HTTPSProxy"),
                CFString::new("disabled.test").into_CFType(),
            ),
            (
                CFString::new("HTTPSPort"),
                CFNumber::from(7891i32).into_CFType(),
            ),
            (
                CFString::new("ExceptionsList"),
                CFArray::from_CFTypes(&[CFString::new("*.excluded.test")]).into_CFType(),
            ),
        ]);
        let settings = parse(&values);
        assert_eq!(settings.http, "http://[::1]:7890");
        assert!(settings.https.is_empty());
        assert_eq!(settings.bypass, "excluded.test");
    }
}
