//! Stable tool identities are shared by registration and admitted-call authorization.
use sailry_protocol::{plugin::Info, tool::Operation};

pub(crate) fn alias(package: &str, tool: &str) -> String {
    let digest = blake3::hash(package.as_bytes()).to_hex();
    format!("plugin_{}_{}", &digest[..16], tool)
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
