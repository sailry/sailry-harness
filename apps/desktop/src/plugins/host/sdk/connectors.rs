//! A package can read its connections and prepare an ordinary browser login.
use super::*;
use sailry_protocol::plugin::connector::{Id, Kind};

impl Host {
    pub(in crate::plugins) fn connectors_module(self: &Arc<Self>) -> HostModule {
        let list = self.clone();
        let types = self.clone();
        let status = self.clone();
        let login = self.clone();
        let cancel = self.clone();
        HostModule::new("sailry/connectors")
            .async_function("list", move |_| list.read_public(Command::ListConnections))
            .async_function("types", move |_| {
                types.read_public(Command::ListConnectorTypes)
            })
            .async_function("status", move |args| {
                let connector = args.string(0)?.to_owned();
                let rows = status.read_public(Command::ListConnections)?;
                Ok(async move {
                    let rows = values::decode(&rows.await?)?;
                    encode(
                        rows.as_array()
                            .and_then(|rows| rows.iter().find(|row| row["connector"] == connector))
                            .cloned()
                            .unwrap_or(Value::Null),
                    )
                })
            })
            .function("login", move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    connector: String,
                    connection: Option<Id>,
                    expected_revision: Option<String>,
                    method: Kind,
                    redirect: Option<String>,
                }
                let input: Input = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|_| HostError::new("invalid connector login"))?;
                login.prepare_public(Command::StartConnectorLogin {
                    package: login.context().package,
                    connector: input.connector,
                    connection: input.connection,
                    expected_revision: input
                        .expected_revision
                        .as_deref()
                        .map(revision)
                        .transpose()?,
                    method: input.method,
                    input: sailry_protocol::plugin::connector::Input {
                        redirect: input.redirect,
                        ..Default::default()
                    },
                })
            })
            .function("cancel", move |args| {
                cancel.prepare_public(Command::CancelConnectorLogin {
                    attempt: args.string(0)?.parse().map_err(HostError::new)?,
                })
            })
            .declarations(include_str!("connectors.d.ts"))
    }
}
