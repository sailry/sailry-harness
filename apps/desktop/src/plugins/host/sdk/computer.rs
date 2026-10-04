//! Permission guidance uses the captured execution Node's public command boundary.
use super::*;
use sailry_protocol::computer::{Permission, Permissions};

impl Host {
    pub(super) fn computer_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let read = self.clone();
        let request = self.clone();
        module
            .async_function("readComputerPermissions", move |_| read.permissions(None))
            .async_function("requestComputerPermission", move |args| {
                let permission = serde_json::from_value(Value::String(args.string(0)?.into()))
                    .map_err(|_| HostError::new("invalid computer permission"))?;
                request.permissions(Some(permission))
            })
    }

    fn permissions(
        self: &Arc<Self>,
        permission: Option<Permission>,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        let node = self.client.target();
        let request = self.read_public(
            permission.map_or(Command::ReadComputerPermissions, |permission| {
                Command::RequestComputerPermission { permission }
            }),
        )?;
        Ok(async move {
            let value = request.await?;
            let permissions: Permissions = serde_json::from_value(decode(&value)?)
                .map_err(|_| HostError::new("invalid computer permissions response"))?;
            if permissions.node != node {
                return Err(HostError::new(
                    "computer permissions belong to another Node",
                ));
            }
            Ok(value)
        })
    }
}
