//! Anonymous Hub discovery and immutable, data-only model definitions.
mod http;
mod huggingface;
mod modelscope;
mod refs;
mod resolve;

#[cfg(test)]
mod tests;

use http::Http;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Fault,
    models::{Definition, Hub, Kind, Page, Search, Source, Version},
};
use std::collections::BTreeMap;

const PAGE_SIZE: u32 = 20;
const METADATA_BYTES: usize = 4 * 1024 * 1024;
const SMALL_BYTES: usize = 32 * 1024 * 1024;

pub(crate) async fn search(
    request: &Search,
    mirror: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Page, Fault> {
    request.validate()?;
    let http = Http::new(request.hub, mirror)?;
    match request.hub {
        Hub::HuggingFace => huggingface::search(&http, request, cancel).await,
        Hub::ModelScope => modelscope::search(&http, request, cancel).await,
    }
}

pub(crate) async fn versions(
    hub: Hub,
    repository: &str,
    mirror: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Vec<Version>, Fault> {
    Source {
        hub,
        repository: repository.to_owned(),
        revision: "main".into(),
    }
    .validate()?;
    let http = Http::new(hub, mirror)?;
    match hub {
        Hub::HuggingFace => huggingface::versions(&http, repository, cancel).await,
        Hub::ModelScope => modelscope::versions(&http, repository, cancel).await,
    }
}

pub(crate) async fn resolve(
    kind: Kind,
    source: &Source,
    mirror: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Definition, Fault> {
    source.validate()?;
    let http = Http::new(source.hub, mirror)?;
    resolve::definition(&http, kind, source, cancel).await
}

struct Repository {
    source: Source,
    name: String,
    license: String,
    files: BTreeMap<String, File>,
}

struct File {
    source: Source,
    path: String,
    bytes: u64,
    sha256: Option<String>,
    blob: Option<String>,
}

fn commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn item(repository: &str, name: Option<&str>, hub: Hub) -> Option<sailry_protocol::models::Item> {
    Source {
        hub,
        repository: repository.into(),
        revision: "main".into(),
    }
    .validate()
    .ok()?;
    let name = name.unwrap_or(repository);
    if name.is_empty() || name.len() > 256 || name.contains('\0') {
        return None;
    }
    Some(sailry_protocol::models::Item {
        repository: repository.into(),
        name: name.into(),
    })
}
