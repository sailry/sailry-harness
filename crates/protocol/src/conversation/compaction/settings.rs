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
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "rejects_disabled_boundaries",
                (|scenario: &'static str| {
                    for percent in [0, 100, 255] {
                        assert!(
                            Settings {
                                percent,
                                ..Settings::default()
                            }
                            .validate()
                            .is_err(),
                            "{scenario}"
                        );
                    }
                    for percent in [1, 80, 99] {
                        assert!(
                            Settings {
                                percent,
                                ..Settings::default()
                            }
                            .validate()
                            .is_ok(),
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "commands_have_durability",
                (|scenario: &'static str| {
                    assert!(
                        !crate::Command::ReadCompactionSettings.durable(),
                        "{scenario}"
                    );
                    assert!(
                        crate::Command::SaveCompactionSettings(Settings::default()).durable(),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
