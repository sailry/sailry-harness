//! Live relay configuration belongs to the existing Link endpoint.
use super::*;
use iroh::{RelayMap, Watcher};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelaySelection {
    Default,
    Custom(Vec<String>),
}

impl RelaySelection {
    pub fn validate(&self) -> Result<(), Fault> {
        self.map().map(|_| ())
    }

    pub(super) fn map(&self) -> Result<RelayMap, Fault> {
        let Self::Custom(urls) = self else {
            return Ok(RelayMode::Default.relay_map());
        };
        if urls.is_empty() || urls.len() > 8 {
            return Err(frame::invalid("expected one to eight relay URLs"));
        }
        for value in urls {
            let url =
                reqwest::Url::parse(value).map_err(|_| frame::invalid("invalid relay URL"))?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(frame::invalid(
                    "relay URLs require HTTPS without credentials",
                ));
            }
        }
        RelayMap::try_from_iter(urls.iter().map(String::as_str))
            .map_err(|_| frame::invalid("invalid relay URL"))
    }
}

impl LinkHandle {
    /// `None` identifies a direct-only endpoint, which has no relay transport.
    pub fn relay_selection(&self) -> Result<Option<RelaySelection>, Fault> {
        Ok(self.core.relay_selection.read().map_err(network)?.clone())
    }

    /// Local configuration only, never an authenticated remote business command.
    /// Success means an iroh handshake on the selected network, not HTTP reachability.
    /// The Link worker completes rollback even if the caller drops its future.
    pub async fn select_relays(
        &self,
        selection: RelaySelection,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let relays = selection.map()?;
        let core = self.core.clone();
        let (reply, response) = oneshot::channel();
        self.core.spawn(Box::pin(async move {
            let result = core.change_relays(selection, relays, stop).await;
            let _ = reply.send(result);
        }))?;
        response.await.map_err(network)?
    }
}

impl Core {
    async fn change_relays(
        &self,
        selection: RelaySelection,
        relays: RelayMap,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let _update = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            update = self.relay_update.lock() => update,
        };
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let previous = self
            .relay_selection
            .read()
            .map_err(network)?
            .clone()
            .ok_or_else(|| frame::invalid("direct-only endpoint has no relay transport"))?;
        let old = previous.map()?;
        self.replace_relays(&old, &relays).await;
        let verified = tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            _ = self.stop.cancelled() => Err(network("closed")),
            result = tokio::time::timeout(PROBE_TIMEOUT, self.relay_ready(&relays)) => {
                result.unwrap_or_else(|_| Err(Fault::new(ErrorCode::Unavailable, "relay connection timed out")))
            }
        };
        let verified = verified.and_then(|_| {
            if stop.is_cancelled() {
                Err(cancelled())
            } else {
                Ok(())
            }
        });
        if let Err(error) = verified {
            self.replace_relays(&relays, &old).await;
            return Err(error);
        }
        *self.relay_selection.write().map_err(network)? = Some(selection);
        Ok(())
    }

    async fn replace_relays(&self, previous: &RelayMap, next: &RelayMap) {
        // Insert before removing so the existing endpoint always has a relay map.
        for config in next.relays::<Vec<_>>() {
            self.endpoint.insert_relay(config.url.clone(), config).await;
        }
        for url in previous.urls::<Vec<_>>() {
            if !next.contains(&url) {
                self.endpoint.remove_relay(&url).await;
            }
        }
    }

    async fn relay_ready(&self, relays: &RelayMap) -> Result<(), Fault> {
        let mut status = self.endpoint.home_relay_status();
        let mut address = self.endpoint.watch_addr();
        loop {
            let statuses = status.get();
            let advertised = address.get();
            if statuses
                .iter()
                .any(|item| relays.contains(item.url()) && item.is_connected())
                && advertised.relay_urls().any(|url| relays.contains(url))
                && advertised.relay_urls().all(|url| relays.contains(url))
            {
                return Ok(());
            }
            if statuses
                .iter()
                .any(|item| relays.contains(item.url()) && item.auth_denied_reason().is_some())
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "relay authentication denied",
                ));
            }
            tokio::select! {
                updated = status.updated() => { updated.map_err(network)?; }
                updated = address.updated() => { updated.map_err(network)?; }
            }
        }
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Unavailable, "relay selection cancelled")
}

#[cfg(test)]
mod tests;
