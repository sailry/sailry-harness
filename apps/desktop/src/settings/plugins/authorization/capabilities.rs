//! Authorization affordances are discovered asynchronously for exact Node resources.
use super::*;
use futures::{StreamExt as _, stream};
use sailry_protocol::plugin::{Info, McpTransport};

pub(in crate::settings) struct Capabilities {
    binding: Binding,
    expected: Vec<(Reference, String)>,
    allowed: Vec<(Reference, String)>,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Capabilities {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Capabilities {
    pub(in crate::settings) fn new(binding: Binding) -> Self {
        Self {
            binding,
            expected: Vec::new(),
            allowed: Vec::new(),
            stop: CancellationToken::new(),
            task: None,
        }
    }

    pub(in crate::settings) fn allows(&self, package: &Reference, server: &str) -> bool {
        self.allowed
            .iter()
            .any(|(key, name)| key == package && name == server)
    }

    pub(in crate::settings) fn accept<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a Info>,
        cx: &mut Context<Self>,
    ) {
        let expected: Vec<_> = entries
            .into_iter()
            .flat_map(|info| {
                info.mcp
                    .iter()
                    .filter(|server| server.transport != McpTransport::Stdio)
                    .map(|server| (info.summary.reference(), server.name.clone()))
            })
            .collect();
        if self.expected == expected {
            return;
        }
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.expected = expected.clone();
        self.allowed.clear();
        let client = self.binding.client.clone();
        let stop = self.stop.clone();
        let job = self.binding.runtime.spawn(async move {
            stream::iter(expected.into_iter().map(|(package, server)| {
                let client = client.clone();
                let stop = stop.clone();
                async move {
                    tokio::select! {
                        _ = stop.cancelled() => None,
                        result = client.execute(client.prepare(Command::ReadMcpAuthorization {
                            package: package.clone(), server: server.clone(),
                        })) => match result {
                            Ok(Output::McpAuthorization(status)) if status.package == package
                                && status.server == server && status.can_authorize => Some((package, server)),
                            _ => None,
                        }
                    }
                }
            })).buffer_unordered(4).filter_map(|value| async move { value }).collect::<Vec<_>>().await
        });
        let stop = self.stop.clone();
        self.task = Some(cx.spawn(async move |owner, cx| {
            let result = job.await;
            _ = owner.update(cx, |owner, cx| {
                if stop.is_cancelled() {
                    return;
                }
                owner.allowed = result.unwrap_or_default();
                cx.notify();
            });
        }));
        cx.notify();
    }
}
