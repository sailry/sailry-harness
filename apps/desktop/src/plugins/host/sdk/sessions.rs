//! Session entry points retain the captured Node and package-owned associations.
use super::*;
use sailry_protocol::{SessionId, plugin::conversation::sdk};

impl Host {
    pub(super) fn sessions_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let projects = self.clone();
        let teams = self.clone();
        let associated = self.clone();
        let start = self.clone();
        module
            .async_function("readProjectCatalog", move |_| {
                projects.read_public(Command::ReadProjectCatalog)
            })
            .async_function("listTeams", move |_| {
                teams.read_scoped(Command::ListTeams, true)
            })
            .async_function("listAssociatedSessions", move |args| {
                let mut association = decode(args.value(0)?)?;
                sdk::association(&mut association, &associated.context.package.name)
                    .map_err(values::fault)?;
                let association = serde_json::from_value(association)
                    .map_err(|error| HostError::new(error.to_string()))?;
                associated.read_scoped(
                    Command::ListAssociatedSessions {
                        association,
                        before: args
                            .get(1)
                            .filter(|value| !matches!(value, HostValue::Null))
                            .map(|value| {
                                value
                                    .as_str()
                                    .ok_or_else(|| HostError::new("invalid session ID"))?
                                    .parse::<SessionId>()
                                    .map_err(HostError::new)
                            })
                            .transpose()?,
                        limit: args
                            .get(2)
                            .filter(|value| !matches!(value, HostValue::Null))
                            .map(|_| args.integer(2))
                            .transpose()?
                            .unwrap_or(32)
                            .try_into()
                            .map_err(|_| HostError::new("invalid session page limit"))?,
                    },
                    true,
                )
            })
            .function("startSession", move |args| {
                let mut draft = decode(args.value(0)?)?;
                if let Some(association) = draft
                    .get_mut("association")
                    .filter(|value| !value.is_null())
                {
                    sdk::association(association, &start.context.package.name)
                        .map_err(values::fault)?;
                }
                start.prepare_public(values::command(
                    json!({"kind":"start_session","data":draft}),
                )?)
            })
    }
}
