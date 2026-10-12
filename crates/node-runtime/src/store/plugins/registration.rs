//! Issued application credentials use the existing revision-checked settings owner.
use super::*;
use crate::plugins::registration::{Credentials, validate};
use sailry_protocol::plugin::{registration::Declaration, settings::SecretUpdate};
use std::collections::BTreeMap;

pub(in crate::store) fn current(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<(Info, Declaration), Fault> {
    let info = settings::current(db, package)?;
    if !info.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "application registration package is disabled",
        ));
    }
    let declaration = info
        .extension
        .as_ref()
        .and_then(|extension| extension.registration.clone())
        .ok_or_else(|| Fault::invalid("plugin does not declare application registration"))?;
    validate(&declaration, info.settings.as_ref())?;
    Ok((info, declaration))
}

pub(in crate::store) fn save(
    db: &Connection,
    package: &plugin::Reference,
    credentials: Credentials,
) -> Result<Info, Fault> {
    let (info, declaration) = current(db, package)?;
    if declaration != credentials.declaration {
        return Err(Fault::invalid(
            "application registration declaration changed",
        ));
    }
    let state = settings::read(db, package)?;
    let mut values = state.values;
    values.insert(
        declaration.client_id,
        serde_json::Value::String(credentials.client_id),
    );
    let mut secrets: BTreeMap<_, _> = info
        .settings
        .as_ref()
        .ok_or_else(|| Fault::invalid("application settings are unavailable"))?
        .properties
        .iter()
        .filter(|(_, field)| field.secret.is_some())
        .map(|(name, _)| (name.clone(), SecretUpdate::Keep))
        .collect();
    secrets.insert(
        declaration.client_secret,
        SecretUpdate::Replace(credentials.client_secret),
    );
    let saved = settings::save(db, package, &values, &secrets)?;
    settings::current(db, &saved.package)
}
