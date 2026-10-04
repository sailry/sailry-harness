//! Assistant policy is part of the ordinary immutable session configuration.
use super::*;
use sailry_protocol::plugin::conversation::{Binding, Resource};
use std::path::Path;

pub(in crate::store) fn validate(
    db: &Connection,
    project: Option<ProjectId>,
    config: &SessionConfig,
) -> Result<(), Fault> {
    let Some(binding) = &config.assistant else {
        return Ok(());
    };
    let info = crate::store::plugins::get(db, &binding.package.name)?
        .filter(|info| info.summary.enabled)
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "assistant plugin is unavailable"))?;
    if info.summary.reference() != binding.package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin assistant version changed",
        ));
    }
    let packages =
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&binding.package))?;
    let declaration = crate::plugins::conversation::declaration(binding, &packages)?;
    if !declaration.resource.matches(project, config.resource) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "conversation resource does not match the plugin assistant",
        ));
    }
    Ok(())
}

pub(super) fn workspace(
    db: &Connection,
    profile: Option<&Path>,
    binding: &Binding,
) -> Result<WorktreeId, Fault> {
    let profile = profile.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin workspace storage is unavailable",
        )
    })?;
    let packages =
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&binding.package))?;
    if crate::plugins::conversation::declaration(binding, &packages)?.resource != Resource::Plugin {
        return Err(commands::invalid(
            "plugin assistant requires its declared resource",
        ));
    }
    let identity =
        serde_json::to_vec(&(&binding.package.name, &binding.id)).map_err(storage_error)?;
    let root = profile
        .join("assistants")
        .join(blake3::hash(&identity).to_hex().as_str())
        .join("workspace");
    super::super::worktrees::scratch(db, &root)
}
