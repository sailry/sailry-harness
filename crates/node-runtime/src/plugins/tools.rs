//! Stable tool identities are shared by registration and admitted-call authorization.
use sailry_protocol::{plugin::Info, tool::Operation};

pub(crate) fn alias(package: &str, tool: &str) -> String {
    let digest = blake3::hash(package.as_bytes()).to_hex();
    format!("plugin_{}_{}", &digest[..16], tool)
}

/// This namespace remains available to exact historical package identities.
pub(crate) fn is_alias(name: &str) -> bool {
    name.strip_prefix("plugin_")
        .and_then(|suffix| suffix.split_once('_'))
        .is_some_and(|(digest, tool)| {
            digest.len() == 16
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && !tool.is_empty()
        })
}

pub(crate) fn operation(package: &Info, name: &str) -> Option<Operation> {
    package.extension.as_ref()?.tools.iter().find_map(|tool| {
        (tool.valid() && alias(&package.summary.name, &tool.name) == name)
            .then(|| {
                tool.operation
                    .or_else(|| tool.handler.as_ref().and_then(|handler| handler.operation))
            })
            .flatten()
    })
}
