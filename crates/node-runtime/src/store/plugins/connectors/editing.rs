//! Editing public metadata does not revoke unchanged authorization.
use super::*;
#[cfg(test)]
mod tests;

pub(super) fn changed(previous: &Record, current: &Record, declaration: &Declaration) -> bool {
    if previous.method != current.method || previous.secrets != current.secrets {
        return true;
    }
    let Some(auth) = declaration
        .auth
        .iter()
        .find(|auth| auth.kind() == current.method)
    else {
        return true;
    };
    let dependencies = match auth {
        Auth::Oauth2 {
            authorize,
            token,
            device,
            ..
        } => serde_json::to_value((authorize, token, device)),
        Auth::Application {
            token,
            registration,
            ..
        } => serde_json::to_value((
            &token.url,
            &token.body,
            registration.as_ref().map(|registration| &registration.flow),
        )),
        _ => Ok(serde_json::Value::Null),
    };
    previous
        .fields
        .keys()
        .chain(current.fields.keys())
        .any(|name| {
            if previous.fields.get(name) == current.fields.get(name) {
                return false;
            }
            let placeholder = format!("{{{name}}}");
            field(auth, name)
                || declaration
                    .inject
                    .iter()
                    .any(|injection| injection.field.as_ref() == Some(name))
                || dependencies
                    .as_ref()
                    .map_or(true, |value| references(value, &placeholder))
                || declaration
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.request.contains(&placeholder))
                || declaration
                    .health
                    .as_ref()
                    .is_some_and(|health| health.request.contains(&placeholder))
        })
}

fn field(auth: &Auth, name: &str) -> bool {
    use sailry_protocol::plugin::connector::{Client, ClientFields};
    let client = |fields: &ClientFields| {
        fields.client_id == name || fields.client_secret.as_deref() == Some(name)
    };
    match auth {
        Auth::Application {
            fields,
            registration,
            ..
        } => {
            client(fields)
                || registration
                    .as_ref()
                    .is_some_and(|registration| client(&registration.issues))
        }
        Auth::Oauth2 {
            client: Client::Fields { fields },
            ..
        }
        | Auth::McpOauth {
            client: Some(Client::Fields { fields }),
            ..
        } => client(fields),
        Auth::Secret { fields } => fields.iter().any(|field| field == name),
        _ => false,
    }
}

fn references(value: &serde_json::Value, placeholder: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value.contains(placeholder),
        serde_json::Value::Array(values) => {
            values.iter().any(|value| references(value, placeholder))
        }
        serde_json::Value::Object(values) => {
            values.values().any(|value| references(value, placeholder))
        }
        _ => false,
    }
}
