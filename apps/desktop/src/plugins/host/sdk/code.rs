//! Language installation events retain the captured Node and ordinary action checks.
use super::*;

impl Host {
    pub(super) fn code_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let owner = self.clone();
        let stream: Arc<tokio::sync::Mutex<Option<Box<dyn sailry_link::Subscription>>>> =
            Default::default();
        module.async_function("nextCodeInstallations", move |_| {
            let permission = owner.read_public(Command::ListCodeInstallations)?;
            let mut stream = stream.clone().try_lock_owned()
                .map_err(|_| HostError::new("an installation update request is already pending"))?;
            let owner = owner.clone();
            Ok(async move {
                permission.await?;
                let runtime = owner.runtime.clone();
                runtime.spawn(async move {
                    let work = async {
                        if stream.is_none() {
                            *stream = Some(owner.client.subscribe_code_installations().await.map_err(values::fault)?);
                        }
                        loop {
                            match stream.as_mut().expect("installation subscription").next().await {
                                Ok(sailry_protocol::Update::CodeInstallations { node, installations })
                                    if node == owner.client.target() => return encode(json!(installations)),
                                Ok(_) => {},
                                Err(error) => {
                                    *stream = None;
                                    return Err(values::fault(error));
                                }
                            }
                        }
                    };
                    tokio::select! {
                        biased;
                        _ = owner.stop.cancelled() => Err(HostError::new("plugin view is closed")),
                        result = work => result,
                    }
                }).await.map_err(|_| HostError::new("installation subscription failed"))?
            })
        })
    }
}
