use super::*;

impl Host {
    pub(super) fn web_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let fetch = self.clone();
        module.async_function("fetchWeb", move |args| {
            let request = serde_json::from_value(decode(args.value(0)?)?)
                .map_err(|error| HostError::new(error.to_string()))?;
            fetch.read_public(Command::FetchWeb(request))
        })
    }
}
