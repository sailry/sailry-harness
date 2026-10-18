//! Resolves executable declarations only from a provider's immutable capture.
use rusqlite::Connection;
use sailry_protocol::{
    ErrorCode, Fault,
    conversation::{Provider, consent::Scope},
    plugin::{Action, model_adapter::Declaration},
};

pub(in crate::store) fn read(
    db: &Connection,
    provider: &Provider,
    messages: bool,
) -> Result<Declaration, Fault> {
    super::presets::verify(db, provider)?;
    if messages {
        super::consent::check(provider, Scope::Messages)?;
    }
    let binding = provider.binding.as_ref().ok_or_else(missing)?;
    let package = binding.package.as_ref().ok_or_else(missing)?;
    let adapter = binding.adapter.as_ref().ok_or_else(missing)?;
    let info = crate::store::agent::plugins::packages(db, std::slice::from_ref(package))?
        .pop()
        .ok_or_else(missing)?;
    let extension = info.extension.ok_or_else(missing)?;
    if !extension.actions.contains(&Action::ProvideModels) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "channel plugin did not declare model access",
        ));
    }
    extension
        .model_adapters
        .into_iter()
        .find(|declaration| {
            declaration.id == adapter.adapter
                && declaration.valid()
                && declaration.capabilities == adapter.capabilities
        })
        .ok_or_else(missing)
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "captured model adapter is unavailable",
    )
}
