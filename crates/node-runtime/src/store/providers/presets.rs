//! Current declarations resolve once at admission; history retains its captured binding.
use crate::store::database::{encode, storage_error};
use sailry_protocol::{
    ErrorCode, Fault,
    conversation::{Provider, presets::*},
    plugin,
};
use turso::{Connection, params};

async fn cached(db: &Connection) -> Result<Vec<Preset>, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM channel_presets WHERE singleton=1")
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row(()).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(Vec::new()),
        Err(error) => return Err(storage_error(error)),
    };
    Ok(
        crate::providers::presets::parse(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .ok()
            .map_or_else(Vec::new, |catalog| catalog.partners),
    )
}

pub(in crate::store) async fn list(
    db: &Connection,
    catalog: Option<&Catalog>,
    unavailable: bool,
) -> Result<Listing, Fault> {
    if let Some(catalog) = catalog {
        let bytes = encode(catalog)?;
        crate::providers::presets::parse(&bytes)?;
        db.execute("INSERT INTO channel_presets(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",params![bytes]).await.map_err(storage_error)?;
    }
    let mut entries = crate::providers::presets::bundled()
        .builtin
        .into_iter()
        .map(|preset| Entry {
            source: Source::Builtin,
            preset,
            provenance: Provenance::Official,
            package: None,
            origin: None,
            requested: Default::default(),
            icon: None,
            login: None,
        })
        .collect::<Vec<_>>();
    for entry in &mut entries {
        if let Auth::Builtin { authentication } = entry.preset.auth {
            entry.login = crate::store::login::model_auth::presentation(db, authentication).await?;
        }
    }
    entries.extend(cached(db).await?.into_iter().map(|preset| Entry {
        source: Source::Partner,
        preset,
        provenance: Provenance::ThirdParty,
        package: None,
        origin: None,
        requested: Default::default(),
        icon: None,
        login: None,
    }));
    for summary in crate::store::plugins::list(db).await? {
        if !summary.available() {
            continue;
        }
        let Some(info) = crate::store::plugins::get(db, &summary.name).await? else {
            continue;
        };
        let provenance = match &info.origin {
            Some(plugin::Origin::Bundled) => Provenance::Bundled,
            Some(plugin::Origin::Online { source, .. })
                if source.repository.trim_end_matches(".git")
                    == "https://github.com/sailry/sailry-plugins" =>
            {
                Provenance::Official
            }
            _ => Provenance::ThirdParty,
        };
        let origin = super::consent::origin(&info);
        if let Some(extension) = info.extension.as_ref() {
            for preset in &extension.model_providers {
                if crate::providers::presets::validate(preset, false, true).is_err() {
                    continue;
                }
                let login = match &preset.auth {
                    Auth::Plugin { flow } => extension
                        .model_auth
                        .iter()
                        .find(|item| &item.id == flow)
                        .map(|item| item.flow.presentation()),
                    _ => None,
                };
                entries.push(Entry {
                    source: Source::Package {
                        name: summary.name.clone(),
                    },
                    preset: preset.clone(),
                    provenance,
                    package: Some(summary.reference()),
                    origin: Some(origin.clone()),
                    requested: super::consent::requested(&info, preset),
                    icon: None,
                    login,
                });
            }
        }
    }
    entries.sort_by(|left, right| {
        fn order(source: &Source) -> (u8, &str) {
            match source {
                Source::Builtin => (0, ""),
                Source::Partner => (1, ""),
                Source::Package { name } => (2, name),
            }
        }
        order(&left.source)
            .0
            .cmp(&order(&right.source).0)
            .then_with(|| left.preset.name.cmp(&right.preset.name))
            .then_with(|| order(&left.source).1.cmp(order(&right.source).1))
            .then_with(|| left.preset.id.cmp(&right.preset.id))
    });
    Ok(Listing {
        entries,
        partners_unavailable: unavailable,
    })
}

pub(in crate::store) async fn resolve(
    db: &Connection,
    reference: &PresetRef,
) -> Result<Entry, Fault> {
    list(db, None, false)
        .await?
        .entries
        .into_iter()
        .find(|entry| entry.source == reference.source && entry.preset.id == reference.id)
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "channel preset is unavailable on this Node",
            )
        })
}

pub(in crate::store) async fn validate(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    if provider.binding.is_some() && !partner_policy(provider)? {
        return Err(Fault::invalid(
            "captured channel bindings cannot be saved as configuration",
        ));
    }
    let Some(reference) = &provider.preset else {
        if provider.api == sailry_protocol::conversation::ModelApi::Plugin {
            return Err(Fault::invalid(
                "custom model protocol requires a plugin preset",
            ));
        }
        return Ok(());
    };
    if matches!(reference.source, Source::Partner) {
        if provider.authentication != sailry_protocol::Authentication::ApiKey
            || provider.api == sailry_protocol::conversation::ModelApi::Plugin
        {
            return Err(Fault::invalid(
                "partner channels require a native key-based protocol",
            ));
        }
        return Ok(());
    }
    let entry = resolve(db, reference).await?;
    let route = entry
        .preset
        .routes
        .iter()
        .find(|route| {
            reference
                .route
                .as_ref()
                .is_none_or(|key| route.key() == *key)
                && route.protocol.api() == provider.api
        })
        .ok_or_else(|| {
            Fault::invalid("channel preset route does not match the selected protocol")
        })?;
    let _ = route;
    let authentication = match entry.preset.auth {
        Auth::ApiKey { .. } | Auth::None => sailry_protocol::Authentication::ApiKey,
        Auth::Builtin { authentication } => authentication,
        Auth::Plugin { .. } => sailry_protocol::Authentication::Plugin,
    };
    if provider.authentication != authentication {
        return Err(Fault::invalid(
            "channel authentication does not match its preset",
        ));
    }
    Ok(())
}

/// Public partner entries are suggestions. Their selected request policy is
/// saved with the channel, rather than resolved from a changing public feed.
pub(in crate::store) async fn save_policy(
    db: &Connection,
    provider: &mut Provider,
    previous: Option<&Provider>,
) -> Result<(), Fault> {
    if !matches!(
        provider.preset.as_ref().map(|reference| &reference.source),
        Some(Source::Partner)
    ) {
        return Ok(());
    }
    partner_policy(provider)?;
    let reference = provider.preset.as_ref().expect("partner reference checked");
    let entry = match resolve(db, reference).await {
        Ok(entry) => Some(entry),
        Err(error) if error.code == ErrorCode::NotConfigured => None,
        Err(error) => return Err(error),
    };
    if let Some(entry) = entry {
        if !entry.preset.routes.iter().any(|route| {
            route.protocol.api() == provider.api
                && reference
                    .route
                    .as_ref()
                    .is_none_or(|key| route.key() == *key)
        }) {
            return Err(Fault::invalid(
                "channel preset route does not match the selected protocol",
            ));
        }
        let key_injection = match entry.preset.auth {
            Auth::ApiKey { header, prefix } if header.is_some() || prefix.is_some() => {
                Some(plugin::model_auth::Injection {
                    header: header.unwrap_or_else(|| "authorization".into()),
                    template: format!("{}{{key}}", prefix.as_deref().unwrap_or("")),
                })
            }
            _ => None,
        };
        let mut policies = if provider.authentication == sailry_protocol::Authentication::ApiKey
            && provider.api == sailry_protocol::conversation::ModelApi::ChatCompletions
        {
            crate::providers::presets::bundled().policies
        } else {
            Vec::new()
        };
        policies.extend(entry.preset.policies);
        provider.binding = Some(Box::new(Binding {
            policies,
            key_injection,
            ..Default::default()
        }));
    } else if let Some(previous) = previous.filter(|previous| previous.preset == provider.preset) {
        provider.binding = previous.binding.clone();
    } else {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "channel preset is unavailable on this Node",
        ));
    }
    Ok(())
}

pub(in crate::store) fn partner_policy(provider: &Provider) -> Result<bool, Fault> {
    if !matches!(
        provider.preset.as_ref().map(|reference| &reference.source),
        Some(Source::Partner)
    ) {
        return Ok(false);
    }
    if let Some(binding) = &provider.binding {
        if binding.package.is_some()
            || binding.origin.is_some()
            || binding.auth.is_some()
            || binding.adapter.is_some()
            || binding.capabilities != Default::default()
        {
            return Err(Fault::invalid(
                "partner policy cannot bind executable package resources",
            ));
        }
        for policy in &binding.policies {
            crate::providers::presets::validate_policy(policy)?;
        }
        if let Some(injection) = &binding.key_injection {
            crate::providers::presets::validate_injection(injection)?;
            if !matches!(
                provider.api,
                sailry_protocol::conversation::ModelApi::ChatCompletions
                    | sailry_protocol::conversation::ModelApi::Responses
                    | sailry_protocol::conversation::ModelApi::Anthropic
            ) {
                return Err(Fault::invalid(
                    "channel protocol does not support custom key headers",
                ));
            }
        }
    }
    Ok(true)
}

pub(in crate::store) async fn discard_invalid_cache(db: &Connection) -> Result<(), Fault> {
    let mut statement = db
        .prepare("SELECT body FROM channel_presets WHERE singleton=1")
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row(()).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(()),
        Err(error) => return Err(storage_error(error)),
    };
    let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
    drop(statement);
    if crate::providers::presets::parse(&body).is_err() {
        db.execute("DELETE FROM channel_presets WHERE singleton=1", ())
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

pub(in crate::store) async fn capture(
    db: &Connection,
    provider: &mut Provider,
) -> Result<(), Fault> {
    bind(db, provider, true).await
}

pub(in crate::store) async fn capture_read(
    db: &Connection,
    provider: &mut Provider,
) -> Result<(), Fault> {
    bind(db, provider, false).await
}

/// Fresh roots resolve current package declarations. Inherited turns retain
/// their original package and configuration during execution.
pub(in crate::store) async fn refresh(
    db: &Connection,
    provider: &mut Provider,
) -> Result<(), Fault> {
    if !partner_policy(provider)? {
        provider.binding = None;
    }
    provider.consent = super::configuration::get(db, provider.id)
        .await?
        .and_then(|current| current.consent);
    capture(db, provider).await
}

async fn bind(db: &Connection, provider: &mut Provider, messages: bool) -> Result<(), Fault> {
    // A profile inherited by a child already contains the parent's exact binding.
    if provider.binding.is_some() {
        verify(db, provider).await?;
        return if messages
            && provider
                .binding
                .as_ref()
                .is_some_and(|binding| binding.adapter.is_some())
        {
            super::consent::check(
                provider,
                sailry_protocol::conversation::consent::Scope::Messages,
            )
        } else {
            Ok(())
        };
    }
    validate(db, provider).await?;
    let mut binding = Binding::default();
    if provider.authentication == sailry_protocol::Authentication::ApiKey
        && provider.api == sailry_protocol::conversation::ModelApi::ChatCompletions
    {
        binding.policies = crate::providers::presets::bundled().policies;
    }
    if let Some(reference) = &provider.preset {
        if matches!(reference.source, Source::Partner) {
            provider.binding = Some(Box::new(binding));
            return Ok(());
        }
        let entry = resolve(db, reference).await?;
        binding.package = entry.package;
        binding.origin = entry.origin;
        binding.capabilities = entry.preset.capabilities;
        if let Auth::ApiKey { header, prefix } = &entry.preset.auth
            && (header.is_some() || prefix.is_some())
        {
            binding.key_injection = Some(plugin::model_auth::Injection {
                header: header.clone().unwrap_or_else(|| "authorization".into()),
                template: format!("{}{{key}}", prefix.as_deref().unwrap_or("")),
            });
        }
        if let Auth::Plugin { flow } = entry.preset.auth {
            binding.auth = Some(plugin::model_auth::Binding { flow });
        }
        binding.adapter = if let Some(adapter) = entry.preset.adapter {
            let package = binding
                .package
                .as_ref()
                .ok_or_else(|| Fault::invalid("model adapter requires its package"))?;
            let info = crate::store::agent::plugins::packages(db, std::slice::from_ref(package))
                .await?
                .pop()
                .ok_or_else(|| Fault::invalid("model adapter package is unavailable"))?;
            let declaration = info
                .extension
                .and_then(|extension| {
                    extension
                        .model_adapters
                        .into_iter()
                        .find(|item| item.id == adapter)
                })
                .ok_or_else(|| Fault::invalid("model adapter declaration is unavailable"))?;
            Some(plugin::model_adapter::Binding {
                adapter,
                capabilities: declaration.capabilities,
            })
        } else {
            None
        };
        binding.policies.extend(entry.preset.policies);
    }
    provider.binding = Some(Box::new(binding));
    verify(db, provider).await?;
    if messages
        && provider
            .binding
            .as_ref()
            .is_some_and(|binding| binding.adapter.is_some())
    {
        super::consent::check(
            provider,
            sailry_protocol::conversation::consent::Scope::Messages,
        )?;
    }
    Ok(())
}

pub(in crate::store) async fn verify(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    partner_policy(provider)?;
    if (provider.api == sailry_protocol::conversation::ModelApi::Plugin)
        != provider
            .binding
            .as_ref()
            .is_some_and(|binding| binding.adapter.is_some())
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "custom model protocol requires its captured adapter",
        ));
    }
    if let Some(binding) = &provider.binding {
        for policy in &binding.policies {
            crate::providers::presets::validate_policy(policy)?;
        }
        if (binding.auth.is_some() || binding.adapter.is_some()) && binding.package.is_none() {
            return Err(Fault::new(
                ErrorCode::Internal,
                "captured channel binding requires its package",
            ));
        }
    }
    if let Some(package) = provider
        .binding
        .as_ref()
        .and_then(|binding| binding.package.as_ref())
    {
        crate::store::agent::plugins::packages(db, std::slice::from_ref(package)).await?;
        if !matches!(provider.preset.as_ref().map(|reference|&reference.source),Some(Source::Package{name}) if name==&package.name)
        {
            return Err(Fault::new(
                ErrorCode::Internal,
                "captured channel package does not match its preset",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
