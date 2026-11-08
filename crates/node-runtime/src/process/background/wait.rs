use super::*;

impl Commands {
    pub(crate) fn wait(
        self: &Arc<Self>,
        session: SessionId,
        id: RequestId,
        wait_ms: u64,
        until: Option<String>,
        closed: CancellationToken,
    ) -> sailry_link::Pending<'static, Result<(), Fault>> {
        let owner = self.clone();
        let until = until.filter(|text| !text.is_empty());
        let mut changes = self.changes.subscribe();
        let channels = {
            let entries = self.entries.lock().unwrap();
            entries
                .get(&id)
                .filter(|entry| entry.info.session == session)
                .map(|entry| {
                    let mut output = entry.output.clone();
                    output.borrow_and_update();
                    (output, entry.retained.subscribe(), entry.promoted)
                })
                .ok_or_else(missing)
        };
        let deadline = tokio::time::sleep(std::time::Duration::from_millis(wait_ms.min(30_000)));
        Box::pin(async move {
            let (mut output, mut retained, promoted) = channels?;
            tokio::pin!(deadline);
            let mut received = false;
            let mut output_open = true;
            loop {
                let snapshot = owner.read(session, id)?;
                if wait_ms == 0
                    || !matches!(snapshot.info.status, Status::Running | Status::Stopping)
                    || until.as_deref().is_some_and(|text| {
                        snapshot.stdout.text.contains(text) || snapshot.stderr.text.contains(text)
                    })
                    || received && until.is_none()
                {
                    return Ok(());
                }
                tokio::select! {
                    biased;
                    _ = closed.cancelled() => {
                        return Err(Fault::new(ErrorCode::Unavailable, "command output wait closed"));
                    }
                    _ = &mut deadline => return Ok(()),
                    result = output.changed(), if output_open => {
                        output_open = result.is_ok();
                        received |= output_open;
                    }
                    result = retained.changed(), if promoted => {
                        if result.is_err() {
                            return Ok(());
                        }
                        received = true;
                    }
                    result = changes.changed() => {
                        if result.is_err() {
                            return Ok(());
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
