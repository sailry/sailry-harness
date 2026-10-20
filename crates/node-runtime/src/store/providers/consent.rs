//! Installed model packages use provider-bound execution, not per-version approval.
use sailry_protocol::{
    ErrorCode, Fault,
    conversation::{
        Provider,
        consent::{Origin, Scope, Scopes},
        presets::Source,
    },
    plugin,
};

pub(in crate::store) fn origin(info: &plugin::Info) -> Origin {
    match &info.origin {
        Some(plugin::Origin::Bundled) => Origin::Bundled,
        Some(plugin::Origin::Online { source, path }) => Origin::Git {
            repository: source.repository.clone(),
            path: path.clone(),
        },
        Some(plugin::Origin::Registry { source }) => Origin::Registry {
            namespace: source.namespace.clone(),
            slug: source.slug.clone(),
        },
        Some(plugin::Origin::Worktree { worktree, path }) => Origin::Worktree {
            worktree: *worktree,
            path: path.clone(),
        },
        _ => Origin::Imported {
            digest: info.summary.digest.clone(),
        },
    }
}

/// Installed imports keep their account owner across content-only updates.
/// Official bundled and repository packages share the same publishing owner.
pub(in crate::store) fn same_origin(package: &str, previous: &Origin, current: &Origin) -> bool {
    if previous == current
        || matches!(
            (previous, current),
            (Origin::Imported { .. }, Origin::Imported { .. })
        )
    {
        return true;
    }
    let official = |origin: &Origin| match origin {
        Origin::Bundled => true,
        Origin::Git { repository, path } => {
            crate::plugins::catalog::official::owns_source(package, repository, path)
        }
        _ => false,
    };
    official(previous) && official(current)
}

pub(in crate::store) fn requested(
    info: &plugin::Info,
    preset: &sailry_protocol::conversation::presets::Preset,
) -> Scopes {
    let credentials =
        if let sailry_protocol::conversation::presets::Auth::Plugin { flow } = &preset.auth {
            info.extension.as_ref().is_some_and(|extension| {
                extension
                    .model_auth
                    .iter()
                    .any(|declaration| declaration.id == *flow && declaration.exchange.is_some())
            })
        } else {
            false
        };
    Scopes {
        messages: preset.adapter.is_some(),
        credentials,
    }
}

/// Captured package ownership remains mandatory. Legacy approval records are
/// retained as data; installation now authorizes declared model operations.
pub(crate) fn check(provider: &Provider, _scope: Scope) -> Result<(), Fault> {
    let binding = provider.binding.as_ref().ok_or_else(denied)?;
    let package = binding.package.as_ref().ok_or_else(denied)?;
    if binding.origin.is_none()
        || !matches!(provider.preset.as_ref().map(|preset| &preset.source),
            Some(Source::Package { name }) if name == &package.name)
    {
        return Err(denied());
    }
    Ok(())
}

fn denied() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "channel plugin does not match its captured provider",
    )
}

#[cfg(test)]
mod tests;
