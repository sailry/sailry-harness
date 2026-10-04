//! Media settings use the captured Node's public configuration commands.
use super::*;

impl Host {
    pub(super) fn media_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let read = self.clone();
        let list = self.clone();
        let save = self.clone();
        module
            .async_function("readMediaSettings", move |_| {
                read.read_public(Command::ReadMediaSettings)
            })
            .async_function("listMediaModels", move |_| {
                list.read_public(Command::ListMediaModels)
            })
            .function("prepareMediaSettings", move |args| {
                let settings = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                save.prepare_public(Command::SaveMediaSettings(settings))
            })
    }
}
