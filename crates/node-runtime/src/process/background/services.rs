//! Announced loopback URLs are candidates, not a scan of listening ports.
use sailry_protocol::process::Service;
use std::sync::LazyLock;

pub(super) fn discover(text: &str, services: &mut Vec<Service>) {
    static ANSI: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
    static URL: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"https?://[^\s<>\"'`\x00-\x1f]+"#).unwrap());
    // A capture may end halfway through the port or path. Wait for a line end.
    let Some(end) = text.rfind(['\n', '\r']) else {
        return;
    };
    let plain = ANSI.replace_all(&text[..=end], "");
    for candidate in URL.find_iter(&plain) {
        if candidate.as_str().len() > 2048 {
            continue;
        }
        let value = candidate
            .as_str()
            .trim_end_matches([')', ']', ',', '.', ';']);
        let Ok(mut url) = url::Url::parse(value) else {
            continue;
        };
        if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "0.0.0.0"))
            || !url.username().is_empty()
            || url.password().is_some()
        {
            continue;
        }
        let Some(port) = url.port_or_known_default().filter(|port| *port != 0) else {
            continue;
        };
        if services.len() >= 16 || services.iter().any(|service| service.port == port) {
            continue;
        }
        url.set_host(Some("127.0.0.1")).unwrap();
        services.push(Service {
            port,
            url: url.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_complete_lines_and_bounds_candidates() {
        let mut services = Vec::new();
        discover("http://localhost:51", &mut services);
        assert!(services.is_empty());
        discover("http://localhost:5173/app\n", &mut services);
        assert_eq!(services[0].port, 5173);
        discover(
            &format!("http://localhost:7000/{}\n", "x".repeat(2048)),
            &mut services,
        );
        assert_eq!(services.len(), 1);
        for port in 8000..8030 {
            discover(&format!("http://localhost:{port}/\n"), &mut services);
        }
        assert_eq!(services.len(), 16);
    }

    #[test]
    fn keeps_paths_and_ignores_unrelated_addresses() {
        let mut services = Vec::new();
        discover(
            "Local: \x1b[36mhttp://localhost:5173/app?q=1\x1b[0m\nhttp://0.0.0.0:5173/ http://127.0.0.1:8000/ http://example.com:1234/ http://localhost.evil:99/ http://user:secret@localhost:30/ http://localhost:0/\n",
            &mut services,
        );
        assert_eq!(
            services,
            vec![
                Service {
                    port: 5173,
                    url: "http://127.0.0.1:5173/app?q=1".into()
                },
                Service {
                    port: 8000,
                    url: "http://127.0.0.1:8000/".into()
                },
            ]
        );
        discover("http://localhost:5173/\n", &mut services);
        assert_eq!(services.len(), 2);
    }
}
