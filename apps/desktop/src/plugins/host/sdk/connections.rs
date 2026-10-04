//! Connection conveniences retain native credentials and original durable requests.
use super::*;
use crate::plugins::credentials;
use gpui_kit::Entity;
use sailry_protocol::{database, ssh};

impl Host {
    pub(in crate::plugins) fn connections_module(
        self: &Arc<Self>,
        credentials: Entity<credentials::Store>,
    ) -> HostModule {
        let databases = self.clone();
        let ssh_profiles = self.clone();
        let database = self.clone();
        let ssh = self.clone();
        let database_credentials = credentials.clone();
        let outcome = self.clone();
        let catalog = self.clone();
        let directory = self.clone();
        let terminals = self.clone();
        let terminal = self.clone();
        HostModule::new("sailry/connections")
            .async_function("listSshTerminals",move |args|terminals.read_public(Command::ListSshTerminals {profile:args.string(0)?.parse().map_err(HostError::new)?}))
            .function("prepareSshTerminal",move |args| {
                terminal.prepare_public(Command::OpenSshTerminal {
                    profile:args.string(0)?.parse().map_err(HostError::new)?,
                    expected_revision:serde_json::from_value(decode(args.value(1)?)?).map_err(|error|HostError::new(error.to_string()))?,
                    launch:ssh::TerminalLaunch {viewport:sailry_protocol::terminal::Viewport {columns:80,rows:24,pixel_width:0,pixel_height:0},
                        appearance:gpui_shell::with_current_app(|cx|crate::theme::terminal(cx)).ok_or_else(||HostError::new("terminal creation requires an active view"))?},
                })
            })
            .async_function("listDatabases",move |_|databases.read_public(Command::ListDatabases))
            .async_function("listSsh",move |_|ssh_profiles.read_public(Command::ListSsh))
            .function("newDatabaseId",|_|Ok(HostValue::from(sailry_protocol::DatabaseId::new().to_string())))
            .function("newSshId",|_|Ok(HostValue::from(sailry_protocol::SshId::new().to_string())))
            .function("prepareDatabase",move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input { profile:database::Profile, secret:Option<String>, testing:bool }
                let input:Input=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;
                let password=input.secret.map(|id|secret(&database_credentials,&id)).transpose()?;
                let expected_revision=input.profile.revision;
                database.prepare_public(if input.testing { Command::TestDatabase { profile:input.profile,expected_revision,password } }
                    else { Command::SaveDatabase { profile:input.profile,expected_revision,password } })
            })
            .function("prepareSsh",move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input { profile:ssh::Profile, credential:Option<Credential> }
                #[derive(serde::Deserialize)]
                #[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
                enum Credential { Password { secret:String }, PrivateKey { secret:String, passphrase:Option<String> } }
                let input:Input=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;
                let credential=input.credential.map(|credential|Ok::<_,HostError>(match credential {
                    Credential::Password { secret:id }=>ssh::Credential::Password { password:secret(&credentials,&id)? },
                    Credential::PrivateKey { secret:id,passphrase }=>ssh::Credential::PrivateKey { key:secret(&credentials,&id)?,passphrase:passphrase.map(|id|secret(&credentials,&id)).transpose()? },
                })).transpose()?;
                ssh.prepare_public(Command::SaveSsh { expected_revision:input.profile.revision, profile:input.profile, credential })
            })
            .async_function("browseDatabase",move |args| {
                let profile=args.string(0)?.parse().map_err(HostError::new)?;
                let expected_revision=serde_json::from_value(decode(args.value(1)?)?).map_err(|error|HostError::new(error.to_string()))?;
                let database=args.get(2).and_then(HostValue::as_str).map(str::to_owned);
                catalog.read_public(Command::BrowseDatabase { profile,expected_revision,database })
            })
            .async_function("browseSshDirectory",move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input { profile:sailry_protocol::SshId,expected_revision:u64,path:String,after:Option<ssh::Cursor> }
                let input:Input=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;
                directory.read_public(Command::BrowseSshDirectory { profile:input.profile,expected_revision:input.expected_revision,path:input.path,after:input.after })
            })
            .async_function("requestOutcome",move |args| {
                outcome.check()?;
                let id=args.string(0)?.parse().map_err(HostError::new)?;
                let request=outcome.requests.lock().map_err(lock_error)?.get(&id).cloned().ok_or_else(||HostError::new("plugin request draft is unavailable"))?;
                let owner=outcome.clone();
                Ok(async move {
                    let runtime=owner.runtime.clone();
                    runtime.spawn(async move {
                        let result=tokio::select! {biased;_ = owner.stop.cancelled()=>return Err(HostError::new("plugin view is closed")),result=owner.client.outcome(&request)=>result.map_err(values::fault)?};
                        let value=match result {
                            RequestOutcome::Completed(result)=>json!({"kind":"completed","data":match *result {Ok(output)=>json!({"Ok":public_output(output)?}),Err(error)=>json!({"Err":error})}}),
                            other=>serde_json::to_value(other).map_err(|error|HostError::new(error.to_string()))?,
                        };
                        encode(value)
                    }).await.map_err(|_|HostError::new("plugin request worker failed"))?
                })
            }).declarations(include_str!("connections.d.ts"))
    }
}
fn secret(
    fields: &Entity<credentials::Store>,
    id: &str,
) -> Result<sailry_protocol::Secret, HostError> {
    gpui_shell::with_current_app(|cx| fields.read(cx).read(id, cx))
        .ok_or_else(|| HostError::new("credential draft requires an active view"))?
}

/// Only the controller projection uses decimal strings; authoritative Node/history bytes stay typed.
pub(super) fn exact_rows(output: &Output, value: &mut Value) {
    let Output::DatabaseOutcome(database::Outcome::Query(result)) = output else {
        return;
    };
    for (row, values) in result.rows.iter().enumerate() {
        for (column, cell) in values.iter().enumerate() {
            if let database::Value::Integer(integer) = cell {
                value["data"]["data"]["rows"][row][column]["value"] = integer.to_string().into();
            }
        }
    }
    value["data"]["data"]["affected_rows"] = result.affected_rows.to_string().into();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_exact_results() {
        let original = Output::DatabaseOutcome(database::Outcome::Query(database::ResultSet {
            columns: vec![
                "large".into(),
                "decimal".into(),
                "text".into(),
                "bytes".into(),
            ],
            rows: vec![vec![
                database::Value::Integer(i64::MAX),
                database::Value::Real("1234567890.12345678901234567890".into()),
                database::Value::Text("资料 🙂".into()),
                database::Value::Blob(vec![0, 255]),
            ]],
            affected_rows: u64::MAX,
            truncated: true,
        }));
        let encoded = serde_json::to_value(&original).unwrap();
        let projected = public_output(original.clone()).unwrap();
        assert_eq!(
            projected["data"]["data"]["rows"][0][0]["value"],
            "9223372036854775807"
        );
        assert_eq!(
            projected["data"]["data"]["affected_rows"],
            "18446744073709551615"
        );
        assert_eq!(
            projected["data"]["data"]["rows"][0][1],
            encoded["data"]["data"]["rows"][0][1]
        );
        assert_eq!(
            projected["data"]["data"]["rows"][0][2],
            encoded["data"]["data"]["rows"][0][2]
        );
        assert_eq!(
            projected["data"]["data"]["rows"][0][3],
            encoded["data"]["data"]["rows"][0][3]
        );
        assert_eq!(serde_json::to_value(original).unwrap(), encoded);
    }
}
