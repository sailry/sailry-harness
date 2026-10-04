//! File operations retain the mounted package's Node and worktree identity.
use super::*;
use sailry_protocol::{FileSearch, WorktreeId};

use crate::plugins::documents::{Controller, entries::EntryAction as Action};
use gpui_kit::Entity;

impl Host {
    pub(super) fn file_module(
        self: &Arc<Self>,
        module: HostModule,
        documents: Option<Entity<Controller>>,
    ) -> HostModule {
        let list = self.clone();
        let search = self.clone();
        let action = self.clone();
        let pending: Arc<Mutex<Option<CancellationToken>>> = Default::default();
        let cancel = pending.clone();
        module.async_function("listDirectory", move |args| {
            let path = args.string(0)?.to_owned();
            let after = args.get(1).filter(|value| !matches!(value, HostValue::Null)).map(|value| serde_json::from_value(decode(value)?).map_err(|error| HostError::new(error.to_string()))).transpose()?;
            list.read_scoped(Command::ListDirectory { worktree: list.file_worktree()?, path, after }, true)
        }).async_function("searchFiles", move |args| {
            let options: FileSearch = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
            let request = search.client.prepare(Command::SearchFiles { worktree: search.file_worktree()?, options }).with_plugin(search.context.clone());
            let stop = search.stop.child_token();
            if let Some(previous) = pending.lock().map_err(lock_error)?.replace(stop.clone()) { previous.cancel(); }
            let host = search.clone();
            Ok(async move {
                let runtime = host.runtime.clone();
                runtime.spawn(async move {
                    let output = tokio::select! { biased; _ = stop.cancelled() => return Err(HostError::new("file search cancelled")), output = host.client.execute(request) => output.map_err(values::fault)? };
                    encode(public_output(output)?.get("data").cloned().unwrap_or(Value::Null))
                }).await.map_err(|_| HostError::new("file search worker failed"))?
            })
        }).function("cancelFileSearch", move |_| {
            if let Some(stop) = cancel.lock().map_err(lock_error)?.take() { stop.cancel(); }
            Ok(HostValue::Null)
        }).function("prepareFileAction", move |args| {
            let worktree = action.file_worktree()?;
            let operation: Action = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
            let controller = documents.as_ref().ok_or_else(|| HostError::new("document scope is unavailable"))?;
            gpui_shell::with_current_app(|cx| {
                let scope = controller.read(cx).scope();
                if operation.paths().iter().any(|path| crate::plugins::file_transfers::dirty(scope, path, cx) || crate::plugins::file_transfers::affects(scope, path, cx)) {
                    return Err(values::fault(sailry_protocol::Fault::new(sailry_protocol::ErrorCode::Busy, "file operation overlaps an unsaved document or pending operation")));
                }
                let value = action.prepare_public(operation.command(worktree))?;
                let id: RequestId = value.as_str().ok_or_else(|| HostError::new("invalid file request"))?.parse().map_err(HostError::new)?;
                let request = action.requests.lock().map_err(lock_error)?.get(&id).cloned().ok_or_else(|| HostError::new("file request is unavailable"))?;
                controller.update(cx, |controller,cx| controller.prepare_entry(operation, request, cx)).map_err(values::fault)?;
                Ok(value)
            }).ok_or_else(|| HostError::new("file action requires an active view"))?

        })
    }

    fn file_worktree(&self) -> Result<WorktreeId, HostError> {
        self.check()?;
        if self.context.surface != sailry_protocol::plugin::desktop::Surface::Workspace {
            return Err(HostError::new("file operations require a workspace"));
        }
        self.context
            .worktree
            .ok_or_else(|| HostError::new("plugin has no worktree scope"))
    }
}
