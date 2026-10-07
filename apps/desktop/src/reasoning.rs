use gpui_kit::SharedString;
use sailry_protocol::Effort;

pub(crate) fn selectable(efforts: &[Effort]) -> bool {
    efforts.iter().any(|effort| *effort != Effort::Default)
}

pub(crate) fn choices(efforts: &[Effort]) -> Vec<Effort> {
    efforts
        .iter()
        .copied()
        .filter(|effort| *effort != Effort::Default)
        .collect()
}

pub(crate) fn label(value: Effort) -> SharedString {
    sailry_protocol::plugin::models::effort_key(value).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_an_explicit_option() {
        assert!(!selectable(&[]));
        assert!(!selectable(&[Effort::Default]));
        assert!(selectable(&[Effort::Low, Effort::High]));
        assert!(selectable(&[Effort::Default, Effort::Budget(1024)]));
    }

    #[test]
    fn keeps_original_names() {
        let before = rust_i18n::locale().to_string();
        for locale in ["en", "zh-CN"] {
            rust_i18n::set_locale(locale);
            for value in [
                "default", "none", "minimal", "low", "medium", "high", "xhigh", "max",
            ] {
                let effort = serde_json::from_value(serde_json::json!(value)).unwrap();
                assert_eq!(label(effort).as_ref(), value);
                assert_eq!(serde_json::to_value(effort).unwrap(), value);
            }
            for tokens in [-1, 0, 1024, 8192] {
                assert_eq!(label(Effort::Budget(tokens)).as_ref(), tokens.to_string());
                assert_eq!(
                    serde_json::to_value(Effort::Budget(tokens)).unwrap(),
                    serde_json::json!({"budget": tokens})
                );
            }
        }
        rust_i18n::set_locale(&before);
    }
}
