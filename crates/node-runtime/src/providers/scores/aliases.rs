pub(super) fn resolve(model: &str) -> Option<&'static str> {
    match model {
        "k2-8" => Some("kimi-for-coding"),
        _ => None,
    }
}
