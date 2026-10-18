//! Current declarations resolve once at admission; history retains its captured binding.
use crate::store::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    ErrorCode, Fault,
    conversation::{Provider, presets::*},
    plugin,
};

fn cached(db: &Connection) -> Result<Vec<Preset>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM channel_presets WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let catalog = body
        .map(|body| crate::providers::presets::parse(&body))
        .transpose()?;
    Ok(catalog.map_or_else(Vec::new, |catalog| catalog.partners))
}

pub(in crate::store) fn list(
    db: &Connection,
    catalog: Option<&Catalog>,
    unavailable: bool,
) -> Result<Listing, Fault> {
    if let Some(catalog) = catalog {
        let bytes = encode(catalog)?;
        crate::providers::presets::parse(&bytes)?;
        db.execute("INSERT INTO channel_presets(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",params![bytes]).map_err(storage_error)?;
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
    entries.extend(cached(db)?.into_iter().map(|preset| Entry {
        source: Source::Partner,
        preset,
        provenance: Provenance::ThirdParty,
        package: None,
        origin: None,
        requested: Default::default(),
        icon: None,
        login: None,
    }));
    for summary in crate::store::plugins::list(db)? {
        if !summary.enabled {
            continue;
        }
        let Some(info) = crate::store::plugins::get(db, &summary.name)? else {
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
                crate::providers::presets::validate(preset, false, true)?;
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
    Ok(Listing {
        entries,
        partners_unavailable: unavailable,
    })
}

pub(in crate::store) fn resolve(db: &Connection, reference: &PresetRef) -> Result<Entry, Fault> {
    list(db, None, false)?
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

pub(in crate::store) fn validate(db: &Connection, provider: &Provider) -> Result<(), Fault> {
    if provider.binding.is_some() {
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
    let entry = resolve(db, reference)?;
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

pub(in crate::store) fn capture(db: &Connection, provider: &mut Provider) -> Result<(), Fault> {
    bind(db, provider, true)
}

pub(in crate::store) fn capture_read(
    db: &Connection,
    provider: &mut Provider,
) -> Result<(), Fault> {
    bind(db, provider, false)
}

/// Only fresh root admission refreshes approval. Inherited turns retain their
/// original approval and package even if configuration changes during execution.
pub(in crate::store) fn refresh(db: &Connection, provider: &mut Provider) -> Result<(), Fault> {
    provider.binding = None;
    provider.consent =
        super::configuration::get(db, provider.id)?.and_then(|current| current.consent);
    capture(db, provider)
}

fn bind(db: &Connection, provider: &mut Provider, messages: bool) -> Result<(), Fault> {
    // A profile inherited by a child already contains the parent's exact binding.
    if provider.binding.is_some() {
        verify(db, provider)?;
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
    validate(db, provider)?;
    let mut binding = Binding::default();
    if provider.authentication == sailry_protocol::Authentication::ApiKey
        && provider.api == sailry_protocol::conversation::ModelApi::ChatCompletions
    {
        binding.policies = crate::providers::presets::bundled().policies;
    }
    if let Some(reference) = &provider.preset {
        let entry = resolve(db, reference)?;
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
        binding.adapter = entry
            .preset
            .adapter
            .map(|adapter| {
                let package = binding
                    .package
                    .as_ref()
                    .ok_or_else(|| Fault::invalid("model adapter requires its package"))?;
                let info =
                    crate::store::agent::plugins::packages(db, std::slice::from_ref(package))?
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
                Ok(plugin::model_adapter::Binding {
                    adapter,
                    capabilities: declaration.capabilities,
                })
            })
            .transpose()?;
        binding.policies.extend(entry.preset.policies);
    }
    provider.binding = Some(Box::new(binding));
    verify(db, provider)?;
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

pub(in crate::store) fn verify(db: &Connection, provider: &Provider) -> Result<(), Fault> {
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
        crate::store::agent::plugins::packages(db, std::slice::from_ref(package))?;
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

pub(in crate::store) fn validate_cache(db: &Connection) -> Result<(), Fault> {
    cached(db).map(|_| ())
}

#[cfg(test)]
mod tests;
