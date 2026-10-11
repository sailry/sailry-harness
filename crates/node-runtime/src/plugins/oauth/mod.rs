//! Fixed account adapters do not execute provider code or accept endpoint overrides.
mod flow;
mod providers;
mod service;
pub(crate) use flow::{Flow, Grant};
pub(crate) use service::{Service, Target};

use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{oauth::Declaration, settings::Schema},
};

pub(crate) fn validate(declaration: &Declaration, schema: Option<&Schema>) -> Result<(), Fault> {
    providers::scopes(declaration)?;
    let schema =
        schema.ok_or_else(|| Fault::invalid("OAuth requires declared credential fields"))?;
    if declaration.credentials.is_empty() || declaration.credentials.len() > 8 {
        return Err(Fault::invalid("invalid OAuth credential fields"));
    }
    for (index, name) in declaration.credentials.iter().enumerate() {
        if declaration.credentials[..index].contains(name) {
            return Err(Fault::invalid("duplicate OAuth credential field"));
        }
        let binding = schema
            .properties
            .get(name)
            .and_then(|field| field.secret.as_ref())
            .ok_or_else(|| Fault::invalid("OAuth credential field is not declared"))?;
        if binding.application.is_some() || binding.oauth.is_some() {
            return Err(Fault::invalid(
                "OAuth credentials cannot also bind application or MCP client secrets",
            ));
        }
        let (origin, header, prefix) = super::http::binding(binding)?;
        if !providers::binding(
            declaration.provider,
            origin.as_str(),
            header.as_str(),
            prefix,
        ) {
            return Err(Fault::invalid(
                "OAuth credential binding does not match its provider",
            ));
        }
    }
    Ok(())
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "plugin account authorization is unavailable",
    )
}

#[cfg(test)]
mod tests;
