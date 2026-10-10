//! Language services are shared execution-Node resources, never a UI runner.
mod catalog;
mod codec;
mod completion;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod fixtures;
mod queries;
pub(crate) use queries::Queries;
mod diagnostics;
mod documents;
mod identity;
mod install;
pub(crate) mod management;
mod network;
mod operations;
mod pool;
mod positions;
mod resolve;
mod rpc;
mod rules;
mod server;
mod subscription;
mod workspace;

pub(crate) use operations::limit;
pub(crate) use operations::{command, description, handles, parameters};
pub(crate) use pool::{Pool, disabled};

pub(crate) fn validate_package(package: &str, version: &str) -> Result<(), sailry_protocol::Fault> {
    install::validate_package(package, version)
}

#[cfg(all(test, unix))]
mod tests;
