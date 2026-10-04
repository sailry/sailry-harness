//! Package-owned result semantics, independent of tool names or result schemas.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultDisplay {
    pub version: u32,
    pub diagnostics: Vec<Diagnostic>,
    pub status: Option<LocalText>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub text: String,
    pub error: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalText {
    pub label: String,
    #[serde(default)]
    pub locales: BTreeMap<String, String>,
}
impl LocalText {
    pub fn label(&self, locale: &str) -> &str {
        self.locales
            .get(locale)
            .map(String::as_str)
            .unwrap_or(&self.label)
    }
    fn valid(&self) -> bool {
        crate::plugin::desktop::Navigation {
            label: self.label.clone(),
            locales: self.locales.clone(),
            icon: None,
        }
        .valid()
    }
}
impl ResultDisplay {
    pub fn from_value(value: &Value) -> Option<Self> {
        let marker = value.get("sailry_result")?;
        if serde_json::to_vec(marker).ok()?.len() > crate::plugin::host::MAX_DATA_BYTES {
            return None;
        }
        let result: Self = serde_json::from_value(marker.clone()).ok()?;
        (result.version == 1 && result.status.as_ref().is_none_or(LocalText::valid))
            .then_some(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_diagnostics_and_authoritative_error() {
        let value = json!({"isError":true,"error":{"code":"unavailable","message":"failed"},
            "sailry_result":{"version":1,"diagnostics":[{"text":"full\ntrace","error":false}],"status":{"label":"Done"}}});
        let display = ResultDisplay::from_value(&value).unwrap();
        assert_eq!(display.diagnostics[0].text, "full\ntrace");
        assert_eq!(value["isError"], true);
        assert_eq!(value["error"]["message"], "failed");
    }
    #[test]
    fn rejects_invalid_markers() {
        for marker in [
            json!({"version":2,"diagnostics":[]}),
            json!({"version":1,"diagnostics":[{"text":"x","error":"no"}]}),
            json!({"version":1,"diagnostics":[],"extra":true}),
        ] {
            assert!(ResultDisplay::from_value(&json!({"sailry_result":marker})).is_none());
        }
        assert!(
            ResultDisplay::from_value(&json!({"sailry_result":{"version":1,"diagnostics":[]}}))
                .is_some()
        );
    }
}
