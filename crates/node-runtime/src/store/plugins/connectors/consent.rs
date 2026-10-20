//! Public differences describe permissions without exposing private MCP configuration.
use super::*;
use sailry_protocol::plugin::connector::{Change, Client, Consent};
fn scopes(auth: &Auth) -> Vec<String> {
    match auth {
        Auth::Oauth2 { scopes, .. } => scopes.clone(),
        Auth::Application {
            registration: Some(registration),
            ..
        } => registration.scopes.clone(),
        _ => Vec::new(),
    }
}
fn client(auth: &Auth) -> String {
    if let Auth::Application { fields, .. } = auth {
        return format!(
            "{} · {}",
            fields.client_id,
            fields.client_secret.as_deref().unwrap_or_default()
        );
    }
    let client = match auth {
        Auth::Oauth2 { client, .. }
        | Auth::McpOauth {
            client: Some(client),
            ..
        } => Some(client),
        _ => None,
    };
    match client {
        Some(Client::Bundled { bundled }) => bundled.client_id.clone(),
        Some(Client::Fields { fields }) => format!(
            "{} · {}",
            fields.client_id,
            fields.client_secret.as_deref().unwrap_or_default()
        ),
        _ => String::new(),
    }
}
fn flow(auth: &Auth) -> String {
    match auth {
        Auth::Oauth2{flow,authorize,token,device,refresh,..}=>serde_json::json!({"kind":flow,"authorize":authorize,"token":token,"device":device,"refresh":refresh}).to_string(),
        Auth::Application{token,registration,..}=>serde_json::json!({"token":token,"registration":registration}).to_string(),
        Auth::McpOauth{server,..}=>server.clone(),_=>String::new()
    }
}
fn targets(bound: &Bound) -> Vec<String> {
    bound
        .inject
        .iter()
        .map(|target| serde_json::json!(target).to_string())
        .collect()
}
pub(super) fn changes(previous: Option<&Bound>, current: Option<&Bound>) -> Option<Consent> {
    let (Some(old), Some(new)) = (previous, current) else {
        return (previous != current).then_some(Consent {
            changes: vec![Change::Source],
        });
    };
    let mut changes = Vec::new();
    if old.origins != new.origins {
        changes.push(Change::Origins {
            previous: old.origins.clone(),
            current: new.origins.clone(),
        });
    }
    let (previous, current) = (scopes(&old.auth), scopes(&new.auth));
    if previous != current {
        changes.push(Change::Scopes { previous, current });
    }
    let (previous, current) = (client(&old.auth), client(&new.auth));
    if previous != current {
        changes.push(Change::Client { previous, current });
    }
    let (previous, current) = (flow(&old.auth), flow(&new.auth));
    if previous != current {
        changes.push(Change::Flow { previous, current });
    }
    let (previous, current) = (targets(old), targets(new));
    if previous != current {
        changes.push(Change::Targets { previous, current });
    }
    if old.targets != new.targets {
        let mut servers = old
            .targets
            .keys()
            .chain(new.targets.keys())
            .cloned()
            .collect::<Vec<_>>();
        servers.sort();
        servers.dedup();
        changes.push(Change::Mcp { servers });
    }
    if old.origin != new.origin {
        changes.push(Change::Source);
    }
    if changes.is_empty() && old != new {
        changes.push(Change::Flow {
            previous: format!("{:?}", old.auth.kind()),
            current: format!("{:?}", new.auth.kind()),
        });
    }
    (!changes.is_empty()).then_some(Consent { changes })
}
