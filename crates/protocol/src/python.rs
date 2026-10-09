//! Execution Node settings for the bundled Python dependency installer.
use crate::Fault;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub index: Option<String>,
}

impl Settings {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.revision > i64::MAX as u64 {
            return Err(Fault::invalid("invalid Python settings revision"));
        }
        if let Some(index) = &self.index {
            let parsed = url::Url::parse(index)
                .map_err(|_| Fault::invalid("invalid Python package index URL"))?;
            if index.len() > 4096
                || !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(Fault::invalid(
                    "Python package index requires an HTTP URL without credentials",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_or_local_indexes() {
        for index in [
            "https://pypi.org/simple",
            "https://pypi.tuna.tsinghua.edu.cn/simple",
            "http://localhost:8080/simple",
        ] {
            Settings {
                index: Some(index.into()),
                ..Settings::default()
            }
            .validate()
            .unwrap();
        }
    }

    #[test]
    fn rejects_credentials_and_invalid_urls() {
        for index in [
            "file:///tmp/wheels",
            "https://user:password@pypi.org/simple",
            "https://pypi.org/simple?token=value",
            "https://pypi.org/simple#fragment",
        ] {
            assert!(
                Settings {
                    index: Some(index.into()),
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}
