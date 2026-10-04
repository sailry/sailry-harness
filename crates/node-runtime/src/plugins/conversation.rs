//! Resolve assistant policy only from the admitted immutable package.
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{
        Info,
        conversation::{Binding, Declaration, Tool},
    },
};

pub(crate) fn declaration<'a>(
    binding: &Binding,
    packages: &'a [Info],
) -> Result<&'a Declaration, Fault> {
    packages
        .iter()
        .find(|package| package.summary.reference() == binding.package)
        .and_then(|package| package.extension.as_ref())
        .and_then(|extension| extension.desktop.as_ref())
        .and_then(|desktop| {
            desktop
                .conversations
                .iter()
                .find(|entry| entry.id == binding.id)
        })
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "plugin assistant declaration is unavailable",
            )
        })
}

/// Imported tools do not add their provider's skills or MCP servers to the assistant.
pub(crate) fn owns_resources(binding: Option<&Binding>, package: &Info) -> bool {
    binding.is_none_or(|binding| package.summary.reference() == binding.package)
}

pub(crate) fn selects(
    binding: Option<&Binding>,
    packages: &[Info],
    provider: &Info,
    name: &str,
) -> bool {
    binding.is_none_or(|binding| {
        declaration(binding, packages).is_ok_and(|declaration| {
            declaration.tools.iter().any(|tool| match tool {
                Tool::Plugin {
                    name: selected,
                    server: None,
                } => owns_resources(Some(binding), provider) && selected == name,
                Tool::Package {
                    package,
                    name: selected,
                } => package == &provider.summary.name && selected == name,
                _ => false,
            })
        })
    })
}
