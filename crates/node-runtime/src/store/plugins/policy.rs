//! Package-owned configuration binds the reusable child execution budget.
use super::*;
use sailry_protocol::plugin::settings::{Field, Kind};

#[cfg(test)]
pub(in crate::store) mod fixture;
#[cfg(test)]
mod tests;

fn field(info: &Info) -> Result<Option<(&str, &Field)>, Fault> {
    let mut fields = info
        .settings
        .iter()
        .flat_map(|schema| &schema.properties)
        .filter(|(_, field)| field.child_concurrency);
    let Some((name, field)) = fields.next() else {
        return Ok(None);
    };
    if fields.next().is_some()
        || field.kind != Kind::String
        || field.secret.is_some()
        || info.extension.as_ref().is_none_or(|extension| {
            extension.scope != plugin::Scope::Host
                || !extension.actions.contains(&plugin::Action::DelegateAgents)
        })
    {
        return Err(Fault::invalid("invalid child execution policy declaration"));
    }
    Ok(Some((name, field)))
}

async fn owner(db: &Connection) -> Result<Option<Info>, Fault> {
    let mut owner = None;
    for summary in list(db).await? {
        let info = required(db, &summary.name).await?;
        if field(&info)?.is_some() {
            if owner.is_some() {
                return Err(Fault::invalid(
                    "child execution policy already has an installed owner",
                ));
            }
            owner = Some(info);
        }
    }
    Ok(owner)
}

pub(super) async fn check(db: &Connection, info: &Info) -> Result<(), Fault> {
    if field(info)?.is_some()
        && owner(db)
            .await?
            .is_some_and(|owner| owner.summary.name != info.summary.name)
    {
        return Err(Fault::invalid(
            "child execution policy already has an installed owner",
        ));
    }
    Ok(())
}

pub(in crate::store) async fn child_limit(db: &Connection) -> Result<Option<u64>, Fault> {
    let Some(info) = owner(db).await? else {
        return Ok(None);
    };
    let (name, _) = field(&info)?.expect("policy owner has a declared field");
    let state = settings::read_admitted(db, &info).await?;
    state
        .values
        .get(name)
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "child execution policy is not configured",
            )
        })
        .and_then(crate::plugins::settings::child_limit)
        .map(Some)
}

pub(super) async fn migrate(db: &Connection, info: &mut Info) -> Result<(), Fault> {
    let Some((name, _)) = field(info)? else {
        return Ok(());
    };
    let name = name.to_owned();
    let Some(retained) = super::super::background_settings::retained(db).await? else {
        return Ok(());
    };
    let mut saved = settings::stored(db, info).await?;
    if info.summary.settings_revision != 0 && saved.values.contains_key(&name) {
        return Ok(());
    }
    let current = info.settings.as_ref().expect("policy has settings");
    saved.values.insert(
        name.clone(),
        serde_json::Value::String(retained.to_string()),
    );
    if let Some(schema) = &mut saved.schema {
        // Do not rebind existing secret slots to a newer schema while migrating
        // this public field. Their original protected bindings remain intact.
        schema
            .properties
            .insert(name.clone(), current.properties[&name].clone());
        if !schema.required.contains(&name) {
            schema.required.push(name);
        }
    } else {
        saved.schema = info.settings.clone();
    }
    settings::persist(db, info, &saved).await
}
