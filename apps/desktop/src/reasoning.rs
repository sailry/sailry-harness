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
    crate::tr(match value {
        Effort::Default => "composer_effort_default",
        Effort::Disabled => "composer_effort_none",
        Effort::Minimal => "composer_effort_minimal",
        Effort::Low => "composer_effort_low",
        Effort::Medium => "composer_effort_medium",
        Effort::High => "composer_effort_high",
        Effort::XHigh => "composer_effort_xhigh",
        Effort::Max => "chat_effort_max",
        Effort::Budget(-1) => "composer_effort_dynamic",
        Effort::Budget(tokens) => {
            return rust_i18n::t!("composer_effort_budget", tokens = tokens)
                .to_string()
                .into();
        }
    })
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
        for value in [
            "default", "none", "minimal", "low", "medium", "high", "xhigh", "max",
        ] {
            let effort = serde_json::from_value(serde_json::json!(value)).unwrap();
            assert_eq!(label(effort).as_ref(), value);
        }
        assert_eq!(label(Effort::Budget(-1)).as_ref(), "dynamic");
        assert_eq!(label(Effort::Budget(1024)).as_ref(), "1024 Token");
    }
}
