//! Default compaction policy owned by the execution Node.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub revision: u64,
    pub percent: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            percent: 80,
        }
    }
}

impl Settings {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 99;

    pub fn validate(&self) -> Result<(), crate::Fault> {
        if !(Self::MIN..=Self::MAX).contains(&self.percent) {
            return Err(crate::Fault::invalid(
                "compaction percentage must be between 1 and 99",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_disabled_boundaries() {
        for percent in [0, 100, 255] {
            assert!(
                Settings {
                    percent,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for percent in [1, 80, 99] {
            assert!(
                Settings {
                    percent,
                    ..Settings::default()
                }
                .validate()
                .is_ok()
            );
        }
    }

    #[test]
    fn commands_have_durability() {
        assert!(!crate::Command::ReadCompactionSettings.durable());
        assert!(crate::Command::SaveCompactionSettings(Settings::default()).durable());
    }
}
