//! Consent displays only the safe differences published by the Node.
use super::*;
use connector::Change;

pub(super) fn detail(connection: &Connection) -> String {
    let target = connection
        .identity
        .as_ref()
        .map_or(connection.connector.as_str(), |identity| {
            identity.name.as_str()
        });
    let mut detail = format!("{target}\n{}", tr("connector_consent_effect"));
    if let Some(consent) = &connection.consent {
        for change in &consent.changes {
            detail.push_str("\n\n");
            detail.push_str(&difference(change));
        }
    }
    detail
}

fn difference(change: &Change) -> String {
    let (key, values) = match change {
        Change::Origins { previous, current } => (
            "connector_changed_origins",
            comparison(&previous.join(", "), &current.join(", ")),
        ),
        Change::Scopes { previous, current } => (
            "connector_changed_scopes",
            comparison(&previous.join(", "), &current.join(", ")),
        ),
        Change::Client { previous, current } => {
            ("connector_changed_client", comparison(previous, current))
        }
        Change::Flow { previous, current } => {
            ("connector_changed_flow", comparison(previous, current))
        }
        Change::Targets { previous, current } => (
            "connector_changed_targets",
            comparison(&previous.join(", "), &current.join(", ")),
        ),
        Change::Mcp { servers } => ("connector_changed_mcp", servers.join(", ")),
        Change::Source => return tr("connector_changed_source").to_string(),
    };
    format!("{}: {values}", tr(key))
}

fn comparison(previous: &str, current: &str) -> String {
    format!(
        "{} → {}",
        if previous.is_empty() { "—" } else { previous },
        if current.is_empty() { "—" } else { current },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn presents_published_changes() {
        for change in [
            Change::Origins {
                previous: vec!["https://old.example".into()],
                current: vec!["https://new.example".into()],
            },
            Change::Scopes {
                previous: vec!["read".into()],
                current: vec!["read".into(), "write".into()],
            },
            Change::Client {
                previous: "old-client".into(),
                current: "new-client".into(),
            },
            Change::Flow {
                previous: "device".into(),
                current: "pkce".into(),
            },
            Change::Targets {
                previous: vec!["old-target".into()],
                current: vec!["new-target".into()],
            },
        ] {
            assert!(difference(&change).contains(" → "));
        }
        assert!(
            difference(&Change::Mcp {
                servers: vec!["documents".into()]
            })
            .ends_with(": documents")
        );
        assert!(!difference(&Change::Source).is_empty());
        assert_eq!(comparison("", "read"), "— → read");
    }
}
