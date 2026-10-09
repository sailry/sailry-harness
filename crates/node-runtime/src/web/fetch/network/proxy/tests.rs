use super::*;

#[test]
fn preserves_environment_precedence_and_exclusions() {
    let settings = Settings {
        http: "http://system.test:80".into(),
        https: "http://system.test:81".into(),
        bypass: "system-exclusion.test".into(),
    };
    let matcher = settings.matcher(|key| match key {
        "ALL_PROXY" => Some("http://all.test:82".into()),
        "HTTPS_PROXY" => Some("http://secure.test:83".into()),
        "NO_PROXY" => Some("excluded.test".into()),
        _ => None,
    });
    assert_eq!(
        matcher
            .intercept(&"http://public.test/".parse().unwrap())
            .unwrap()
            .uri()
            .host(),
        Some("all.test")
    );
    assert_eq!(
        matcher
            .intercept(&"https://public.test/".parse().unwrap())
            .unwrap()
            .uri()
            .host(),
        Some("secure.test")
    );
    assert!(
        matcher
            .intercept(&"https://child.excluded.test/".parse().unwrap())
            .is_none()
    );
}

#[test]
fn uses_system_settings_without_environment_changes() {
    let settings = Settings {
        http: "http://system.test:80".into(),
        https: "http://system.test:81".into(),
        bypass: "excluded.test".into(),
    };
    let matcher = settings.matcher(|_| None);
    assert_eq!(
        matcher
            .intercept(&"https://public.test/".parse().unwrap())
            .unwrap()
            .uri()
            .port_u16(),
        Some(81)
    );
    assert!(
        matcher
            .intercept(&"http://excluded.test/".parse().unwrap())
            .is_none()
    );
}
