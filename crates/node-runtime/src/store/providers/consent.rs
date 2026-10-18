//! Approval is owned by a configured provider, never a global permission registry.
use rusqlite::Connection;
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

/// An unchanged approval can survive an implementation update at the same
/// installation origin. New approvals must name the exact displayed package.
pub(in crate::store) fn stamp(
    db: &Connection,
    provider: &mut Provider,
    previous: Option<&Provider>,
) -> Result<(), Fault> {
    let Some(mut approval) = provider.consent.take() else {
        return Ok(());
    };
    let Some(Source::Package { name }) = provider.preset.as_ref().map(|preset| &preset.source)
    else {
        return Err(Fault::invalid("only plugin channels use package approval"));
    };
    let info = crate::store::plugins::get(db, name)?
        .filter(|info| info.summary.enabled)
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "channel plugin is unavailable"))?;
    let reference = info.summary.reference();
    if approval.package.name != reference.name {
        return Err(denied());
    }
    let origin = origin(&info);
    let retained = previous.and_then(|provider| provider.consent.as_ref()) == Some(&approval)
        && approval.origin.as_ref() == Some(&origin);
    if !retained && approval.package != reference {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "channel plugin changed before approval",
        ));
    }
    approval.origin = Some(origin);
    provider.consent = Some(approval);
    Ok(())
}

pub(crate) fn check(provider: &Provider, scope: Scope) -> Result<(), Fault> {
    let binding = provider.binding.as_ref().ok_or_else(denied)?;
    let package = binding.package.as_ref().ok_or_else(denied)?;
    captured(
        provider.consent.as_deref(),
        package,
        binding.origin.as_ref(),
        scope,
    )
}

/// Only values frozen into the admitted provider are used here. A later grant
/// cannot elevate an old turn, and later revocation applies to new admission.
pub(crate) fn captured(
    approval: Option<&sailry_protocol::conversation::consent::Approval>,
    package: &plugin::Reference,
    origin: Option<&Origin>,
    scope: Scope,
) -> Result<(), Fault> {
    let approval = approval.ok_or_else(denied)?;
    if approval.package.name != package.name
        || approval.origin.as_ref() != origin
        || origin.is_none()
        || !approval.scopes.allows(scope)
    {
        return Err(denied());
    }
    Ok(())
}

fn denied() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "channel plugin access has not been approved",
    )
}

#[cfg(test)]
mod tests;
