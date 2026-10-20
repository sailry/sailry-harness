//! Anonymous Hub discovery and immutable, data-only model definitions.
mod http;
mod huggingface;
mod modelscope;
mod refs;
pub(crate) mod registry;
mod resolve;
pub(crate) mod runtime;
#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;

use futures::{StreamExt, stream};
use http::Http;
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
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
    #[cfg(any(test, feature = "test-support"))] fixture: Option<reqwest::Url>,
) -> Result<Page, Fault> {
    request.validate()?;
    let http = http(
        request.hub,
        mirror,
        #[cfg(any(test, feature = "test-support"))]
        fixture,
    )?;
    compatible(&http, request, cancel).await
}

async fn compatible(
    http: &Http,
    request: &Search,
    cancel: &CancellationToken,
) -> Result<Page, Fault> {
    let mut request = request.clone();
    let mut partial = false;
    for _ in 0..3 {
        let listing = match request.hub {
            Hub::HuggingFace => huggingface::search(http, &request, cancel).await?,
            Hub::ModelScope => modelscope::search(http, &request, cancel).await?,
        };
        partial |= listing.partial;
        let (kind, hub) = (request.kind, request.hub);
        let outcomes =
            stream::iter(listing.items)
                .map(|candidate| async move {
                    resolve::inspect(http, kind, hub, candidate, cancel).await
                })
                .buffered(4)
                .collect::<Vec<_>>()
                .await;
        let mut items = Vec::new();
        for result in outcomes {
            match result {
                Ok(item) => items.push(item),
                Err(error) if error.code == ErrorCode::Cancelled => return Err(error),
                Err(error)
                    if matches!(
                        error.code,
                        ErrorCode::InvalidRequest
                            | ErrorCode::NotFound
                            | ErrorCode::PermissionDenied
                            | ErrorCode::NotConfigured
                    ) => {}
                Err(_) => partial = true,
            }
        }
        let next_cursor = listing.next_cursor;
        if next_cursor.is_some() && next_cursor == request.cursor {
            return Err(Fault::invalid("model pagination cursor did not advance"));
        }
        if !items.is_empty() || next_cursor.is_none() {
            return Ok(Page {
                items,
                next_cursor,
                partial,
            });
        }
        request.cursor = next_cursor;
    }
    Ok(Page {
        items: Vec::new(),
        next_cursor: request.cursor,
        partial,
    })
}

pub(crate) async fn versions(
    hub: Hub,
    repository: &str,
    mirror: Option<&str>,
    cancel: &CancellationToken,
    #[cfg(any(test, feature = "test-support"))] fixture: Option<reqwest::Url>,
) -> Result<Vec<Version>, Fault> {
    Source {
        hub,
        repository: repository.to_owned(),
        revision: "main".into(),
    }
    .validate()?;
    let http = http(
        hub,
        mirror,
        #[cfg(any(test, feature = "test-support"))]
        fixture,
    )?;
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
    #[cfg(any(test, feature = "test-support"))] fixture: Option<reqwest::Url>,
) -> Result<Definition, Fault> {
    source.validate()?;
    let http = http(
        source.hub,
        mirror,
        #[cfg(any(test, feature = "test-support"))]
        fixture,
    )?;
    resolve::definition(&http, kind, source, cancel).await
}

/// Updates the same OCR profile, retaining the detector, recognizer and dictionary roles.
pub(crate) async fn refresh_ocr(
    previous: &Definition,
    mirror: Option<&str>,
    cancel: &CancellationToken,
    #[cfg(any(test, feature = "test-support"))] fixture: Option<reqwest::Url>,
) -> Result<Definition, Fault> {
    previous.validate()?;
    if previous.format.kind() != Kind::Ocr {
        return Err(Fault::invalid("OCR model format required"));
    }
    let http = http(
        previous.source.hub,
        mirror,
        #[cfg(any(test, feature = "test-support"))]
        fixture,
    )?;
    resolve::ocr::refresh(&http, previous, cancel).await
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

fn item(repository: &str, name: Option<&str>, hub: Hub) -> Option<Candidate> {
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
    Some(Candidate {
        repository: repository.into(),
        name: name.into(),
    })
}

#[derive(Debug)]
struct Listing {
    items: Vec<Candidate>,
    next_cursor: Option<String>,
    partial: bool,
}

#[derive(Debug)]
struct Candidate {
    repository: String,
    name: String,
}

fn http(
    hub: Hub,
    mirror: Option<&str>,
    #[cfg(any(test, feature = "test-support"))] fixture: Option<reqwest::Url>,
) -> Result<Http, Fault> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(base) = fixture {
        return Ok(Http::fixture(base));
    }
    Http::new(hub, mirror)
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub(crate) struct Fixtures(std::sync::Mutex<Vec<(Hub, reqwest::Url)>>);

#[cfg(any(test, feature = "test-support"))]
impl Fixtures {
    pub(crate) fn set(&self, hub: Hub, endpoint: &str) -> Result<(), Fault> {
        let endpoint = reqwest::Url::parse(endpoint)
            .map_err(|_| Fault::invalid("invalid model catalog fixture URL"))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.host_str().is_some_and(|host| {
                host.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            })
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Fault::invalid(
                "model catalog fixture requires a loopback URL without credentials",
            ));
        }
        let mut fixtures = self.0.lock().expect("model fixture lock is valid");
        fixtures.retain(|(stored, _)| *stored != hub);
        fixtures.push((hub, endpoint));
        Ok(())
    }
    pub(crate) fn get(&self, hub: Hub) -> Option<reqwest::Url> {
        self.0
            .lock()
            .expect("model fixture lock is valid")
            .iter()
            .find(|(stored, _)| *stored == hub)
            .map(|(_, url)| url.clone())
    }
}
