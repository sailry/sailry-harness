use super::*;

impl Transfers {
    pub(super) fn reserve(
        self: &Arc<Self>,
        caller: NodeId,
        root: PathBuf,
        closed: CancellationToken,
    ) -> Result<(Lease, OwnedSemaphorePermit, CancellationToken), Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        let stream = StreamId::new();
        let cancel = closed.child_token();
        let mut registry = self.entries.lock().unwrap();
        if registry.closed {
            return Err(unavailable());
        }
        if registry
            .entries
            .values()
            .filter(|entry| entry.caller == caller)
            .count()
            >= PER_CALLER
        {
            return Err(busy());
        }
        registry.entries.insert(
            stream,
            Entry {
                caller,
                root,
                cancel: cancel.clone(),
                state: State::Preparing,
                expires: Instant::now() + OPEN_TIMEOUT,
            },
        );
        Ok((
            Lease {
                transfers: self.clone(),
                stream,
                armed: true,
            },
            permit,
            cancel,
        ))
    }

    pub(super) fn ready(
        self: &Arc<Self>,
        lease: &mut Lease,
        resource: Resource,
        cancel: CancellationToken,
    ) -> Result<(), Fault> {
        {
            let mut registry = self.entries.lock().unwrap();
            let entry = registry
                .entries
                .get_mut(&lease.stream)
                .ok_or_else(cancelled)?;
            entry.state = State::Ready(resource);
            entry.expires = Instant::now() + OPEN_TIMEOUT;
        }
        lease.armed = false;
        self.schedule_expiry(lease.stream, cancel);
        Ok(())
    }

    pub(super) fn schedule_expiry(self: &Arc<Self>, stream: StreamId, cancel: CancellationToken) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    if let Some(transfers) = weak.upgrade() { transfers.finish(stream); }
                },
                _ = tokio::time::sleep(OPEN_TIMEOUT) => {
                    if let Some(transfers) = weak.upgrade() { transfers.expire(stream); }
                }
            }
        });
    }

    pub(crate) fn cancel(&self, caller: NodeId, stream: StreamId) -> Result<(), Fault> {
        let mut registry = self.entries.lock().unwrap();
        match registry.entries.get(&stream) {
            Some(entry) if entry.caller != caller => Err(missing()),
            Some(entry) if matches!(entry.state, State::Finishing) => Err(Fault::new(
                ErrorCode::Conflict,
                "upload publication has started; inspect the command result",
            )),
            _ => {
                if let Some(entry) = registry.entries.remove(&stream) {
                    entry.cancel.cancel();
                }
                Ok(())
            }
        }
    }

    pub(crate) fn remove(&self, root: &std::path::Path) {
        let mut registry = self.entries.lock().unwrap();
        registry.entries.retain(|_, entry| {
            if entry.root.starts_with(root) {
                entry.cancel.cancel();
                false
            } else {
                true
            }
        });
    }

    pub(crate) fn shutdown(&self) {
        let mut registry = self.entries.lock().unwrap();
        registry.closed = true;
        for (_, entry) in std::mem::take(&mut registry.entries) {
            entry.cancel.cancel();
        }
    }

    pub(super) fn finish(&self, stream: StreamId) {
        if let Some(entry) = self.entries.lock().unwrap().entries.remove(&stream) {
            entry.cancel.cancel();
        }
    }

    pub(super) fn expire(&self, stream: StreamId) {
        let mut registry = self.entries.lock().unwrap();
        if registry.entries.get(&stream).is_some_and(|entry| {
            matches!(entry.state, State::Ready(_) | State::Uploaded(_))
                && entry.expires <= Instant::now()
        }) && let Some(entry) = registry.entries.remove(&stream)
        {
            entry.cancel.cancel();
        }
    }
}
