//! Host SDK handlers retain the captured view, location and request identity.
use super::*;
use gpui_shell::{HostArguments, HostResult};

pub(super) fn invoke(
    args: &HostArguments,
    owner: &WeakEntity<Panel>,
    state: &Entity<State>,
    host: &Arc<Host>,
) -> Result<impl Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    let intent = serde_json::from_value(Value::String(args.string(0)?.to_owned()))
        .map_err(|error| HostError::new(error.to_string()))?;
    let value = args.get(1).map(decode).transpose()?.unwrap_or(Value::Null);
    request(
        owner,
        state.clone(),
        host.clone(),
        None,
        Operation::Invoke(intent, Some(value)),
    )
}

pub(super) fn available(
    args: &HostArguments,
    owner: &WeakEntity<Panel>,
    state: &Entity<State>,
    host: &Arc<Host>,
) -> Result<impl Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    let intent = serde_json::from_value(Value::String(args.string(0)?.to_owned()))
        .map_err(|error| HostError::new(error.to_string()))?;
    request(
        owner,
        state.clone(),
        host.clone(),
        None,
        Operation::Invoke(intent, None),
    )
}

pub(super) fn read(state: &Entity<State>, host: &Arc<Host>) -> HostResult {
    host.check()?;
    gpui_shell::with_current_app(|cx| {
        let state = state.read(cx);
        state.check(host, None).map_err(HostError::new)?;
        encode(state.changes.borrow().clone())
    })
    .ok_or_else(|| HostError::new("location requires an active view"))?
}

pub(super) fn changes(
    args: &HostArguments,
    state: &Entity<State>,
    host: &Arc<Host>,
) -> Result<impl Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    host.check()?;
    let seen = args.string(0)?.to_owned();
    let mut state = gpui_shell::with_current_app(|cx| {
        let state = state.read(cx);
        if !state.read {
            return Err(HostError::new("location read is unavailable"));
        }
        Ok(state.changes.subscribe())
    })
    .ok_or_else(|| HostError::new("location requires an active view"))??;
    let stop = host.stop_token();
    Ok(async move {
        loop {
            let value = state.borrow_and_update().clone();
            if value["cursor"].as_str() != Some(&seen) {
                return encode(value);
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                result = state.changed() => result.map_err(|_| HostError::new("location is closed"))?,
            }
        }
    })
}

pub(super) fn select(
    args: &HostArguments,
    owner: &WeakEntity<Panel>,
    state: &Entity<State>,
    host: &Arc<Host>,
) -> Result<impl Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    let cursor = args.string(0)?.to_owned();
    let selection: Selection = serde_json::from_value(decode(args.value(1)?)?)
        .map_err(|error| HostError::new(error.to_string()))?;
    let prepared = selection.request.map(|id| host.prepared(id)).transpose()?;
    let operation = Operation::Select(selection);
    let operation = prepared.map_or_else(
        || operation.clone(),
        |request| Operation::Confirm(request, Box::new(operation.clone())),
    );
    request(owner, state.clone(), host.clone(), Some(cursor), operation)
}

pub(super) fn drafts(
    args: &HostArguments,
    owner: &WeakEntity<Panel>,
    state: &Entity<State>,
    host: &Arc<Host>,
) -> Result<impl Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    let worktree = args
        .string(0)?
        .parse()
        .map_err(|_| HostError::new("invalid worktree ID"))?;
    let operation = Operation::Drafts(worktree);
    let receipt = args
        .get(1)
        .map(|value| {
            serde_json::from_value::<Option<RequestId>>(decode(value)?)
                .map_err(|error| HostError::new(error.to_string()))
        })
        .transpose()?
        .flatten();
    let operation = if let Some(id) = receipt {
        Operation::Confirm(host.prepared(id)?, Box::new(operation))
    } else {
        operation
    };
    request(owner, state.clone(), host.clone(), None, operation)
}

pub(super) fn release(
    args: &HostArguments,
    owner: &WeakEntity<Panel>,
    state: &Entity<State>,
    host: &Arc<Host>,
) -> Result<impl Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    let worktree = args
        .string(0)?
        .parse()
        .map_err(|_| HostError::new("invalid worktree ID"))?;
    let id = args
        .string(1)?
        .parse::<RequestId>()
        .map_err(|_| HostError::new("invalid request ID"))?;
    let prepared = host.prepared(id)?;
    request(
        owner,
        state.clone(),
        host.clone(),
        None,
        Operation::Release(worktree, prepared),
    )
}
