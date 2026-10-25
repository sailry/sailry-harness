//! Rebuildable code structure uses ordinary scoped Node reads.
use super::*;

impl Host {
    pub(super) fn code_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let search = self.clone();
        let outline = self.clone();
        module
            .async_function("searchCode", move |args| {
                let query = args
                    .get(0)
                    .map(decode)
                    .transpose()?
                    .unwrap_or_else(|| json!({}));
                let query = serde_json::from_value(query)
                    .map_err(|error| HostError::new(error.to_string()))?;
                search.read_scoped(
                    Command::SearchCode {
                        worktree: search
                            .context
                            .worktree
                            .ok_or_else(|| HostError::new("plugin has no worktree scope"))?,
                        query,
                    },
                    true,
                )
            })
            .async_function("outlineCode", move |args| {
                outline.read_scoped(
                    Command::OutlineCode {
                        worktree: outline
                            .context
                            .worktree
                            .ok_or_else(|| HostError::new("plugin has no worktree scope"))?,
                        path: args.string(0)?.into(),
                    },
                    true,
                )
            })
    }
}
