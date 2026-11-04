//! Provider quota observations from the execution Node's own model responses.
use crate::{CredentialRef, ProviderId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// Hundredths of a percent; 10,000 represents 100%.
    pub used_basis_points: u32,
    pub reset_at_ms: Option<u64>,
}

impl Window {
    /// A passed reset makes the observation unavailable, not an estimated refill.
    pub fn available_at(&self, now_ms: u64) -> bool {
        self.reset_at_ms.is_none_or(|reset| reset > now_ms)
    }

    pub fn remaining_basis_points(&self) -> u32 {
        10_000_u32.saturating_sub(self.used_basis_points)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub provider: ProviderId,
    pub credential: CredentialRef,
    /// Revision of the credential used for the observed response.
    pub revision: u64,
    pub observed_ms: u64,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "expires_without_estimating_refill",
                (|scenario: &'static str| {
                    let window = Window {
                        used_basis_points: 4_250,
                        reset_at_ms: Some(100),
                    };
                    assert!(window.available_at(99), "{scenario}");
                    assert!(!window.available_at(100), "{scenario}");
                    assert!(!window.available_at(101), "{scenario}");
                    assert_eq!(window.used_basis_points, 4_250, "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "preserves_observations_without_reset",
                (|scenario: &'static str| {
                    let window = Window {
                        used_basis_points: 0,
                        reset_at_ms: None,
                    };
                    assert!(window.available_at(u64::MAX), "{scenario}");
                }),
            ),
            (
                "preserves_overage_without_negative_remaining",
                (|scenario: &'static str| {
                    let window = Window {
                        used_basis_points: 10_050,
                        reset_at_ms: None,
                    };
                    assert_eq!(window.remaining_basis_points(), 0, "{scenario}");
                    assert_eq!(window.used_basis_points, 10_050, "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
