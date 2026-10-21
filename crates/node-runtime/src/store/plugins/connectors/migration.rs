//! Move only explicit saved configuration before applying current form defaults.
use super::*;

/// Only the current settings revision is moved; historical captures remain untouched.
pub(in crate::store) fn needed(db: &Connection, candidate: &Info) -> Result<bool, Fault> {
    let Some(info) = get(db, &candidate.summary.name)? else {
        return Ok(false);
    };
    if info.summary.settings_revision == 0 {
        return Ok(false);
    }
    let saved = settings::stored(db, &info)?;
    let records = records(db)?;
    Ok(registry::declarations(candidate).iter().any(|declaration| {
        !records
            .iter()
            .any(|record| record.plugin == info.summary.name && record.connector == declaration.id)
            && configured(&saved, declaration)
    }))
}

pub(in crate::store::plugins) fn migrate(
    db: &Connection,
    profile: Option<&std::path::Path>,
    info: &mut Info,
) -> Result<bool, Fault> {
    let declarations = registry::declarations(info);
    if declarations.is_empty() || info.summary.settings_revision == 0 {
        return Ok(false);
    }
    let mut stored = settings::stored(db, info)?;
    let mut changed = false;
    for declaration in declarations {
        if !configured(&stored, &declaration) {
            continue;
        }
        if records(db)?
            .iter()
            .any(|record| record.plugin == info.summary.name && record.connector == declaration.id)
        {
            continue;
        }
        let method=if stored.github.is_some() && !declaration.fields.iter().any(|field| field.secret && stored.slots.get(&field.name).is_some_and(Option::is_some)) && declaration.auth.iter().any(|auth|auth.kind()==Kind::LocalCli){Kind::LocalCli}
            else if stored.oauth.as_ref().and_then(|authorization|authorization.grant.as_ref()).is_some() && declaration.auth.iter().any(|auth|auth.kind()==Kind::Oauth2){Kind::Oauth2}
            else if declaration.auth.iter().any(|auth|matches!(auth,Auth::McpOauth{server,..} if stored.authorizations.get(server).is_some_and(|grant|grant.credentials.is_some()))){Kind::McpOauth}
            else if declaration.auth.iter().any(|auth|auth.kind()==Kind::Application){Kind::Application}
            else {declaration.auth.iter().find(|auth| auth.kind() == Kind::Secret).or_else(|| declaration.auth.first()).map(Auth::kind).ok_or_else(|| Fault::invalid("connector authentication method is unavailable"))?};
        let mut record = Record {
            id: RequestId::new(),
            plugin: info.summary.name.clone(),
            connector: declaration.id.clone(),
            revision: 1,
            auth_revision: 1,
            method,
            enabled: true,
            fields: BTreeMap::new(),
            secrets: BTreeMap::new(),
            grant: None,
            grant_revision: 1,
            identity: None,
            bound: bound(db, profile, info, &declaration, method)?,
            state: State::NotConnected,
            checked_at_ms: None,
            login: None,
        };
        for field in &declaration.fields {
            if field.secret {
                if let Some(secret) = stored.slots.get(&field.name).cloned().flatten() {
                    record.secrets.insert(field.name.clone(), secret);
                }
            } else if let Some(value) = stored
                .values
                .get(&field.name)
                .and_then(serde_json::Value::as_str)
            {
                record.fields.insert(field.name.clone(), value.into());
            }
        }
        input(&mut record, &declaration, &Input::default())?;
        match method {
            Kind::LocalCli => {
                if let Some(identity) = &stored.github {
                    record.identity = Some(Identity {
                        name: identity.login.clone(),
                        id: identity.login.clone(),
                    });
                    record.secrets.clear();
                    record.state = State::Connected;
                }
            }
            Kind::Oauth2 => {
                if let Some(grant) = stored
                    .oauth
                    .as_ref()
                    .and_then(|authorization| authorization.grant.as_ref())
                {
                    record
                        .fields
                        .insert("client_id".into(), grant.configuration.client_id.clone());
                    if let Some(secret) = &grant.configuration.client_secret {
                        record
                            .secrets
                            .insert("client_secret".into(), secret.clone());
                    }
                    if let Some(tenant) = &grant.configuration.tenant {
                        record.fields.insert("tenant".into(), tenant.clone());
                    }
                    record.grant = Some(Grant {
                        access: grant.access_token.clone(),
                        refresh: grant.refresh_token.clone(),
                        expires_at_ms: Some(grant.expires_at_ms),
                        extra: BTreeMap::new(),
                    });
                    record.state = State::Connected;
                }
            }
            Kind::McpOauth => {
                if let Some(Auth::McpOauth { server, .. }) =
                    declaration.auth.iter().find(|auth| auth.kind() == method)
                    && let Some(credentials) = stored
                        .authorizations
                        .get(server)
                        .and_then(|grant| grant.credentials.clone())
                {
                    record.grant = Some(Grant {
                        access: credentials,
                        refresh: None,
                        expires_at_ms: None,
                        extra: BTreeMap::new(),
                    });
                    record.state = State::Connected;
                }
            }
            Kind::Secret => {
                if let Some(Auth::Secret { fields }) =
                    declaration.auth.iter().find(|auth| auth.kind() == method)
                    && fields
                        .iter()
                        .all(|field| record.secrets.contains_key(field))
                {
                    record.state = State::Connected;
                }
            }
            Kind::Application => {
                if declaration
                    .fields
                    .iter()
                    .filter(|field| !field.optional)
                    .all(|field| {
                        if field.secret {
                            record.secrets.contains_key(&field.name)
                        } else {
                            record.fields.contains_key(&field.name)
                        }
                    })
                {
                    record.state = State::Connected;
                }
            }
        }
        if record.fields.is_empty()
            && record.secrets.is_empty()
            && record.grant.is_none()
            && record.identity.is_none()
        {
            continue;
        }
        save(db, &record)?;
        for field in &declaration.fields {
            stored.values.remove(&field.name);
            stored.slots.remove(&field.name);
        }
        if method == Kind::Oauth2 {
            if let Some(grant) = stored.oauth.as_ref().and_then(|saved| saved.grant.as_ref()) {
                for name in &grant.declaration.credentials {
                    stored.slots.remove(name);
                }
            }
            stored.oauth = None;
        }
        if method == Kind::LocalCli {
            stored.github = None;
        }
        if let Some(Auth::McpOauth { server, .. }) =
            declaration.auth.iter().find(|auth| auth.kind() == method)
        {
            stored.authorizations.remove(server);
        }
        changed = true;
    }
    if changed {
        stored.schema = info.settings.clone();
        settings::persist(db, info, &stored)?;
    }
    Ok(changed)
}

fn configured(saved: &settings::Stored, declaration: &Declaration) -> bool {
    declaration.fields.iter().any(|field| {
        saved
            .values
            .get(&field.name)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty())
            || saved
                .slots
                .get(&field.name)
                .and_then(Option::as_ref)
                .is_some_and(|value| !value.expose().is_empty())
    }) || declaration.auth.iter().any(|auth| match auth {
        Auth::Oauth2 { .. } => saved
            .oauth
            .as_ref()
            .and_then(|saved| saved.grant.as_ref())
            .is_some(),
        Auth::McpOauth { server, .. } => saved
            .authorizations
            .get(server)
            .is_some_and(|grant| grant.credentials.is_some()),
        Auth::LocalCli { .. } => saved.github.is_some(),
        _ => false,
    })
}
