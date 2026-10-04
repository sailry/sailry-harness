//! Role settings retain the captured Node and existing revision-checked commands.
use super::*;

impl Host {
    pub(super) fn roles_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let list = self.clone();
        let put = self.clone();
        let remove = self.clone();
        module
            .async_function("listRoles", move |_| list.read_public(Command::ListRoles))
            .function("newRoleId", |_| {
                Ok(HostValue::from(sailry_protocol::RoleId::new().to_string()))
            })
            .function("prepareRole", move |args| {
                let role = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let expected_revision = serde_json::from_value(decode(args.value(1)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                put.prepare_public(Command::PutRole {
                    role,
                    expected_revision,
                })
            })
            .function("prepareRemoveRole", move |args| {
                let id = args
                    .string(0)?
                    .parse()
                    .map_err(|_| HostError::new("invalid role ID"))?;
                let expected_revision = serde_json::from_value(decode(args.value(1)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                remove.prepare_public(Command::RemoveRole {
                    role: id,
                    expected_revision,
                })
            })
    }
}
