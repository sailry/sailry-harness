//! Native fixture storage and Node error projection.
#[path = "database/error.rs"]
mod error;
#[path = "database/native.rs"]
mod native;

pub use error::fault;
pub use native::{close, open};
