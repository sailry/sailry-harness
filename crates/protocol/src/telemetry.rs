//! Profile-local choices for anonymous product diagnostics and counters.
use crate::Fault;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub revision: u64,
    #[serde(default = "enabled")]
    pub errors: bool,
    #[serde(default = "enabled")]
    pub metrics: bool,
    #[serde(default = "Uuid::new_v4")]
    pub installation_id: Uuid,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            errors: true,
            metrics: true,
            installation_id: Uuid::new_v4(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.revision > i64::MAX as u64 || self.installation_id.is_nil() {
            return Err(Fault::invalid("telemetry settings are invalid"));
        }
        Ok(())
    }
}

/// The controller can change consent without supplying an installation identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub revision: u64,
    pub errors: bool,
    pub metrics: bool,
}

fn enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_to_enabled_anonymous_choices() {
        let settings: Settings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(settings.revision, 0);
        assert!(settings.errors);
        assert!(settings.metrics);
        assert_eq!(settings.installation_id.get_version_num(), 4);
        settings.validate().unwrap();
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(serde_json::from_value::<Settings>(value).unwrap(), settings);
    }

    #[test]
    fn rejects_invalid_identity_and_values() {
        for value in [
            json!({"errors":"yes"}),
            json!({"metrics":null}),
            json!({"installation_id":"invalid"}),
        ] {
            assert!(serde_json::from_value::<Settings>(value).is_err());
        }
        let mut settings = Settings {
            installation_id: Uuid::nil(),
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        settings.installation_id = Uuid::new_v4();
        settings.revision = i64::MAX as u64 + 1;
        assert!(settings.validate().is_err());
        assert!(
            serde_json::from_value::<Update>(json!({
                "revision":0,"errors":false,"metrics":false,
                "installation_id":Uuid::new_v4()
            }))
            .is_err()
        );
    }
}
