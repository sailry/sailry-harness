//! Scoped callbacks let domain services report real work without owning task state.
use sailry_protocol::installation::{Phase, Progress};
use std::sync::Arc;

pub(crate) type Reporter = Arc<dyn Fn(Phase, Option<Progress>) + Send + Sync>;
