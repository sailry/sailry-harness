//! Per-peer dial serialization; the global cache lock never covers network I/O.
use super::*;
use std::sync::Mutex as StdMutex;

pub(super) struct Slot {
    dial: Mutex<()>,
    stop: CancellationToken,
    pub(super) cached: StdMutex<Option<CachedConnection>>,
}

impl Slot {
    fn new() -> Self {
        Self {
            dial: Mutex::new(()),
            stop: CancellationToken::new(),
            cached: StdMutex::new(None),
        }
    }

    fn stale(&self) -> bool {
        self.cached.lock().is_ok_and(|cached| {
            cached
                .as_ref()
                .is_none_or(|cached| cached.connection.close_reason().is_some())
        })
    }

    pub(super) fn close(&self, code: u32, reason: &[u8]) {
        self.stop.cancel();
        if let Ok(mut cached) = self.cached.lock()
            && let Some(cached) = cached.take()
        {
            cached.connection.close(code.into(), reason);
        }
    }
    async fn wait<T>(
        &self,
        pending: impl std::future::Future<Output = Result<T, Fault>>,
    ) -> Result<T, Fault> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(Fault::new(ErrorCode::Conflict,"peer connection changed")),
            result = pending => result,
        }
    }
}

impl Core {
    async fn slot(&self, peer: NodeId) -> Result<Arc<Slot>, Fault> {
        let mut outgoing = self.outgoing.lock().await;
        // An in-flight dial or a waiter holds an Arc. Never evict its registered
        // slot and let that waiter start an uncounted replacement connection.
        outgoing.retain(|_, slot| Arc::strong_count(slot) > 1 || !slot.stale());
        if let Some(slot) = outgoing.get(&peer) {
            if slot.stop.is_cancelled() {
                return Err(Fault::new(ErrorCode::Conflict, "peer connection changed"));
            }
            return Ok(slot.clone());
        }
        if outgoing.len() >= MAX_CONNECTIONS {
            return Err(Fault::new(
                ErrorCode::Busy,
                "Link connection capacity exhausted",
            ));
        }
        let slot = Arc::new(Slot::new());
        outgoing.insert(peer, slot.clone());
        Ok(slot)
    }

    async fn registered(&self, peer: NodeId, slot: &Arc<Slot>) -> Result<(), Fault> {
        if self.stop.is_cancelled() {
            return Err(network("closed"));
        }
        if slot.stop.is_cancelled() {
            return Err(Fault::new(ErrorCode::Conflict, "peer connection changed"));
        }
        if !self.trusted(peer) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "peer is not paired",
            ));
        }
        if !self
            .outgoing
            .lock()
            .await
            .get(&peer)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            return Err(Fault::new(ErrorCode::Conflict, "peer connection changed"));
        }
        Ok(())
    }

    fn publish(
        &self,
        slot: &Slot,
        address: EndpointAddr,
        connection: Connection,
        software_version: Option<String>,
        zstd: bool,
    ) -> Result<(), Fault> {
        let mut cached = slot.cached.lock().map_err(network)?;
        if let Some(previous) = cached.take()
            && previous.connection.stable_id() != connection.stable_id()
        {
            previous
                .connection
                .close(0u32.into(), b"peer address changed");
        }
        let (updates, telemetry) = watch::channel(connection::snapshot(&connection));
        let sampler = tokio::spawn(connection::monitor(
            connection.clone(),
            updates,
            self.stop.clone(),
        ));
        *cached = Some(CachedConnection {
            address,
            connection,
            software_version,
            zstd,
            telemetry,
            sampler,
        });
        Ok(())
    }

    pub(super) async fn connection(
        &self,
        address: &EndpointAddr,
        execution: bool,
    ) -> Result<(Connection, bool), Fault> {
        let peer = NodeId(*address.id.as_bytes());
        if !self.trusted(peer) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "peer is not paired",
            ));
        }
        let slot = self.slot(peer).await?;
        let _dial = slot.dial.lock().await;
        self.registered(peer, &slot).await?;
        let target = self.current_address(address)?;
        {
            let cached = slot.cached.lock().map_err(network)?;
            if let Some(cached) = cached.as_ref().filter(|cached| {
                cached.connection.close_reason().is_none()
                    && target.addrs.is_subset(&cached.address.addrs)
            }) {
                if execution {
                    crate::compatibility::check(
                        self.handler.product_version(),
                        cached.software_version.as_deref(),
                    )?;
                }
                return Ok((cached.connection.clone(), cached.zstd));
            }
        }
        let connection = slot
            .wait(async {
                self.wait(self.endpoint.connect(target.clone(), ALPN))
                    .await?
                    .map_err(network)
            })
            .await?;
        let result = async {
            let info = slot.wait(self.inspect_connection(&connection)).await?;
            if execution {
                crate::compatibility::check(
                    self.handler.product_version(),
                    info.software_version.as_deref(),
                )?;
            }
            let _update = self.trust_update.lock().await;
            self.registered(peer, &slot).await?;
            if self.current_address(address)? != target {
                return Err(Fault::new(ErrorCode::Conflict, "peer address changed"));
            }
            self.publish(
                &slot,
                target,
                connection.clone(),
                info.software_version,
                info.zstd,
            )?;
            Ok(info.zstd)
        }
        .await;
        match result {
            Ok(compressed) => Ok((connection, compressed)),
            Err(error) => {
                connection.close(0u32.into(), b"peer connection rejected");
                Err(error)
            }
        }
    }

    pub(super) async fn replace_address(&self, address: EndpointAddr) -> Result<(), Fault> {
        let peer = NodeId(*address.id.as_bytes());
        if !self.trusted(peer) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "peer is not paired",
            ));
        }
        let slot = self.slot(peer).await?;
        let _dial = slot.dial.lock().await;
        self.registered(peer, &slot).await?;
        let previous = self.addresses.read().map_err(network)?.get(&peer).cloned();
        let connection = slot
            .wait(async {
                self.wait(self.endpoint.connect(address.clone(), ALPN))
                    .await?
                    .map_err(network)
            })
            .await?;
        let result = async {
            let info = slot.wait(self.inspect_connection(&connection)).await?;
            crate::compatibility::check(
                self.handler.product_version(),
                info.software_version.as_deref(),
            )?;
            let _update = self.trust_update.lock().await;
            self.registered(peer, &slot).await?;
            if self.addresses.read().map_err(network)?.get(&peer) != previous.as_ref() {
                return Err(Fault::new(ErrorCode::Conflict, "peer address changed"));
            }
            self.store.remember(address.clone()).await?;
            self.addresses
                .write()
                .map_err(network)?
                .insert(peer, address.clone());
            self.publish(
                &slot,
                address,
                connection.clone(),
                info.software_version,
                info.zstd,
            )
        }
        .await;
        if result.is_err() {
            connection.close(0u32.into(), b"peer connection rejected");
        }
        result
    }
}

#[cfg(test)]
mod tests;
