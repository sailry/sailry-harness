//! Standalone controller bootstrap; no business database or execution handlers.
//! The old Code 67ae9fa0 mobile runtime is a behavior reference only: its global
//! product registries and UI projections are not carried into this Link owner.
mod store;

use crate::*;
use sailry_protocol::{ErrorCode, NodeId, Request};
use std::path::Path;

impl Link {
    /// Mobile/controller-only endpoint. Desktop uses its existing Node's Link.
    pub async fn controller(path: impl AsRef<Path>, scope: NetworkScope) -> Result<Self, Fault> {
        let path = path.as_ref().to_owned();
        let (identity, store) = tokio::task::spawn_blocking(move || store::Store::open(&path))
            .await
            .map_err(storage)??;
        Self::bind(&identity, scope, Arc::new(NoExecution), Arc::new(store)).await
    }
}

fn storage(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("controller storage: {error}"),
    )
}

struct NoExecution;
impl Handler for NoExecution {
    fn execution_available(&self) -> bool {
        false
    }
    fn dispatch(&self, _: NodeId, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async { Err(no_execution()) })
    }
    fn subscribe(
        &self,
        _: NodeId,
        _: sailry_protocol::Topic,
    ) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(no_execution()) })
    }
}
fn no_execution() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "controller has no execution service",
    )
}
