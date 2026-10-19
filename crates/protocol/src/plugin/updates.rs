//! Read-only update inspection; installation remains an explicit durable command.
use super::{Info, Summary, catalog};
use crate::Fault;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "package", rename_all = "snake_case")]
pub enum Candidate {
    Package(Box<Info>),
    /// Changed official contents; installation still captures its own immutable selection.
    Official(Box<catalog::Entry>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub current: Summary,
    pub available: Option<Candidate>,
    /// Uploaded packages without a reusable source require another explicit selection.
    pub manual: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub reports: Vec<Report>,
    pub failures: Vec<Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub name: String,
    pub error: Fault,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_payloads() {
        for value in [
            json!({
                "kind": "package",
                "package": {
                    "summary": {
                        "name": "fixture", "revision": 1, "digest": "digest",
                        "settings_revision": 0, "enabled": true,
                        "version": null, "description": null
                    },
                    "icon": null, "skill": null, "extension": null,
                    "skills": [], "mcp": [], "settings": null, "issues": []
                }
            }),
            json!({
                "kind": "official",
                "package": {
                    "id": "fixture", "name": "Fixture", "version": "0.1.0",
                    "description": null, "display": null, "icon": null,
                    "repository": null, "bundled": true,
                    "provides_models": false, "permissions": []
                }
            }),
        ] {
            let candidate: Candidate = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(candidate).unwrap(), value);
        }
    }
}
