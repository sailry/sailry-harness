use super::{File, PAGE_SIZE, Repository, commit, digest, http::Http, item, refs::Refs};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Hub, Page, Search, Source, Version},
};
use serde_json::Value;
use std::collections::BTreeMap;

const RESULT_LIMIT: u32 = 3000;

pub(super) async fn search(
    http: &Http,
    request: &Search,
    cancel: &CancellationToken,
) -> Result<Page, Fault> {
    let page = request
        .cursor
        .as_deref()
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| Fault::invalid("ModelScope pagination cursor is invalid"))?
        .unwrap_or(1);
    if page == 0 || page > RESULT_LIMIT / PAGE_SIZE {
        return Err(Fault::invalid(
            "ModelScope pagination exceeds its result limit",
        ));
    }
    let mut url = http.url(&["openapi", "v1", "models"]);
    url.query_pairs_mut()
        .append_pair("search", request.query.trim())
        .append_pair("page_number", &page.to_string())
        .append_pair("page_size", &PAGE_SIZE.to_string());
    // Task labels vary across ONNX exports and can be absent for Model2Vec.
    // Resolve the selected candidate against the supported runtime format.
    let (value, _) = http.json(url, cancel).await?;
    let data = data(&value)?;
    let rows = data
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| Fault::invalid("ModelScope model listing is invalid"))?;
    let total = data
        .get("total_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| Fault::invalid("ModelScope result count is unavailable"))?;
    if data.get("page_number").and_then(Value::as_u64) != Some(u64::from(page))
        || data.get("page_size").and_then(Value::as_u64) != Some(u64::from(PAGE_SIZE))
    {
        return Err(Fault::invalid(
            "ModelScope returned a different result page",
        ));
    }
    if rows.len() > PAGE_SIZE as usize {
        return Err(Fault::invalid(
            "ModelScope listing exceeds its requested page size",
        ));
    }
    let items: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            item(
                row.get("id")?.as_str()?,
                row.get("display_name").and_then(Value::as_str),
                Hub::ModelScope,
            )
        })
        .collect();
    let more = u64::from(page * PAGE_SIZE) < total;
    let capped = (page + 1) * PAGE_SIZE > RESULT_LIMIT;
    let partial = items.len() != rows.len() || more && capped;
    Ok(Page {
        items,
        next_cursor: (more && !capped).then(|| (page + 1).to_string()),
        partial,
    })
}

pub(super) async fn versions(
    http: &Http,
    repository: &str,
    cancel: &CancellationToken,
) -> Result<Vec<Version>, Fault> {
    match Refs::read(http, repository, cancel).await {
        Ok(refs) => Ok(refs.versions()),
        Err(error) if error.code == ErrorCode::Cancelled => Err(error),
        Err(_) => {
            let revisions = revisions(http, repository, cancel).await?;
            Ok(revisions
                .branches
                .into_iter()
                .chain(revisions.tags)
                .map(|name| Version {
                    revision: name.clone(),
                    name,
                })
                .collect())
        }
    }
}

pub(super) async fn repository(
    http: &Http,
    source: &Source,
    cancel: &CancellationToken,
) -> Result<Repository, Fault> {
    let url = http.repository(&["openapi", "v1", "models"], &source.repository, &[]);
    let (info, _) = http.json(url, cancel).await?;
    let info = data(&info)?;
    if info.get("private").and_then(Value::as_bool) == Some(true)
        || info.get("gated").and_then(Value::as_bool) == Some(true)
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "model repository requires authentication",
        ));
    }
    let mut pinned = source.clone();
    let snapshot = if commit(&source.revision) {
        pinned.revision.make_ascii_lowercase();
        false
    } else {
        match Refs::read(http, &source.repository, cancel).await {
            Ok(refs) => {
                pinned.revision = refs
                    .resolve(&source.revision)
                    .ok_or_else(|| {
                        Fault::new(
                            ErrorCode::NotFound,
                            "ModelScope branch or tag was not found",
                        )
                    })?
                    .into();
                false
            }
            Err(error) if error.code == ErrorCode::Cancelled => return Err(error),
            Err(_) => {
                let revisions = revisions(http, &source.repository, cancel).await?;
                let default = info
                    .get("default_branch")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        (revisions.branches.len() == 1).then(|| revisions.branches[0].as_str())
                    });
                if default != Some(source.revision.as_str())
                    || !revisions.branches.contains(&source.revision)
                    || revisions.tags.contains(&source.revision)
                {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "ModelScope could not pin this branch or tag; select a full commit",
                    ));
                }
                // The visible source remains the selected branch. Each asset pins
                // its own immutable file revision from this one current tree.
                true
            }
        }
    };
    let mut url = http.repository(
        &["api", "v1", "models"],
        &source.repository,
        &["repo", "files"],
    );
    url.query_pairs_mut()
        .append_pair("Revision", &pinned.revision)
        .append_pair("Recursive", "True");
    let (value, _) = http.json(url, cancel).await?;
    let rows = data(&value)?
        .get("Files")
        .and_then(Value::as_array)
        .ok_or_else(|| Fault::invalid("ModelScope file listing is invalid"))?;
    if rows.len() >= RESULT_LIMIT as usize {
        return Err(Fault::invalid("ModelScope file listing may be truncated"));
    }
    let mut files = BTreeMap::new();
    for row in rows {
        if row.get("Type").and_then(Value::as_str) == Some("tree") {
            continue;
        }
        let path = row
            .get("Path")
            .and_then(Value::as_str)
            .ok_or_else(|| Fault::invalid("ModelScope file path is invalid"))?;
        let bytes = row
            .get("Size")
            .and_then(Value::as_u64)
            .ok_or_else(|| Fault::invalid("ModelScope file size is unavailable"))?;
        let sha256 = row
            .get("Sha256")
            .and_then(Value::as_str)
            .filter(|value| digest(value))
            .ok_or_else(|| Fault::invalid("ModelScope file checksum is unavailable"))?;
        let mut file_source = pinned.clone();
        if snapshot {
            file_source.revision = row
                .get("Revision")
                .and_then(Value::as_str)
                .filter(|value| commit(value))
                .ok_or_else(|| Fault::invalid("ModelScope file revision is not pinned"))?
                .to_ascii_lowercase();
        }
        if files
            .insert(
                path.into(),
                File {
                    source: file_source,
                    path: path.into(),
                    bytes,
                    sha256: Some(sha256.to_ascii_lowercase()),
                    blob: None,
                },
            )
            .is_some()
        {
            return Err(Fault::invalid(
                "ModelScope file listing contains duplicates",
            ));
        }
    }
    Ok(Repository {
        source: pinned,
        name: info
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(&source.repository)
            .into(),
        license: info
            .get("license")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        files,
    })
}

struct Revisions {
    branches: Vec<String>,
    tags: Vec<String>,
}

async fn revisions(
    http: &Http,
    repository: &str,
    cancel: &CancellationToken,
) -> Result<Revisions, Fault> {
    let url = http.repository(&["api", "v1", "models"], repository, &["revisions"]);
    let (value, _) = http.json(url, cancel).await?;
    let map = data(&value)?
        .get("RevisionMap")
        .ok_or_else(|| Fault::invalid("ModelScope version listing is invalid"))?;
    let read = |key| -> Result<Vec<String>, Fault> {
        map.get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| Fault::invalid("ModelScope version listing is invalid"))?
            .iter()
            .map(|row| {
                let name = row
                    .get("Revision")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Fault::invalid("ModelScope version name is invalid"))?;
                Source {
                    hub: sailry_protocol::models::Hub::ModelScope,
                    repository: repository.into(),
                    revision: name.into(),
                }
                .validate()?;
                Ok(name.into())
            })
            .collect()
    };
    Ok(Revisions {
        branches: read("Branches")?,
        tags: read("Tags")?,
    })
}

fn data(value: &Value) -> Result<&Value, Fault> {
    if value.get("success").and_then(Value::as_bool) == Some(false)
        || value.get("Success").and_then(Value::as_bool) == Some(false)
    {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "ModelScope rejected the catalog request",
        ));
    }
    value
        .get("data")
        .or_else(|| value.get("Data"))
        .filter(|data| data.is_object())
        .ok_or_else(|| Fault::invalid("ModelScope returned invalid model metadata"))
}
