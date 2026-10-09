use super::Settings;

pub(super) fn settings() -> Settings {
    let Ok(values) = windows_registry::CURRENT_USER
        .open("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
    else {
        return Settings::default();
    };
    if values.get_u32("ProxyEnable").unwrap_or_default() != 1 {
        return Settings::default();
    }
    let proxy = values.get_string("ProxyServer").unwrap_or_default();
    let mut settings = Settings::default();
    if proxy.contains('=') {
        for entry in proxy.split(';') {
            if let Some((scheme, endpoint)) = entry.trim().split_once('=') {
                match scheme {
                    "http" => settings.http = endpoint.to_owned(),
                    "https" => settings.https = endpoint.to_owned(),
                    _ => {}
                }
            }
        }
    } else {
        settings.http = proxy.clone();
        settings.https = proxy;
    }
    settings.bypass = values
        .get_string("ProxyOverride")
        .unwrap_or_default()
        .split(';')
        .map(|entry| entry.trim().trim_start_matches("*."))
        .filter(|entry| !entry.is_empty() && *entry != "<local>")
        .collect::<Vec<_>>()
        .join(",");
    settings
}
