use super::{File, Listing, PAGE_SIZE, Repository, commit, digest, http::Http, item};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Hub, Search, Source, Version},
};
use std::collections::BTreeMap;

pub(super) async fn search(
    http: &Http,
    request: &Search,
    cancel: &CancellationToken,
) -> Result<Listing, Fault> {
    let mut url = http.url(&["api", "models"]);
    {
        let mut query = url.query_pairs_mut();
        if request.query.trim().is_empty() {
            query.append_pair("filter", "onnx");
            query.append_pair("sort", "downloads");
            query.append_pair("direction", "-1");
            if request.kind == sailry_protocol::models::Kind::Speech {
                query.append_pair("search", "sensevoice");
            }
        } else {
            query.append_pair("search", request.query.trim());
        }
        query.append_pair("limit", &PAGE_SIZE.to_string());
        // Compatible ONNX and static exports can lack pipeline metadata.
        // Compatibility is established by resolution and installation.
        if let Some(cursor) = &request.cursor {
            query.append_pair("cursor", cursor);
        }
    }
    let (value, headers) = http.json(url.clone(), cancel).await?;
    let rows = value
        .as_array()
        .ok_or_else(|| Fault::invalid("Hugging Face model listing is invalid"))?;
    if rows.len() > PAGE_SIZE as usize {
        return Err(Fault::invalid(
            "Hugging Face listing exceeds its requested page size",
        ));
    }
    let items: Vec<_> = rows
        .iter()
        .filter_map(|row| item(row.get("id")?.as_str()?, None, Hub::HuggingFace))
        .collect();
    let mut next_cursor = None;
    for link in headers.get_all(reqwest::header::LINK) {
        let link = link
            .to_str()
            .map_err(|_| Fault::invalid("Hugging Face pagination is invalid"))?;
        for part in link.split(',') {
            let mut fields = part.trim().split(';');
            let address = fields.next().unwrap_or_default().trim();
            if !fields.any(|field| field.trim() == "rel=\"next\"") {
                continue;
            }
            let address = address
                .strip_prefix('<')
                .and_then(|value| value.strip_suffix('>'))
                .ok_or_else(|| Fault::invalid("Hugging Face pagination is invalid"))?;
            let next = reqwest::Url::parse(address)
                .map_err(|_| Fault::invalid("Hugging Face pagination is invalid"))?;
            if next.origin() != url.origin() || next.path() != url.path() {
                return Err(Fault::invalid(
                    "Hugging Face pagination changed its endpoint",
                ));
            }
            let cursor = next
                .query_pairs()
                .find_map(|(key, value)| (key == "cursor").then(|| value.into_owned()))
                .filter(|value| !value.is_empty() && value.len() <= 8192)
                .ok_or_else(|| Fault::invalid("Hugging Face pagination cursor is invalid"))?;
            next_cursor = Some(cursor);
        }
    }
    let partial = items.len() != rows.len();
    Ok(Listing {
        items,
        next_cursor,
        partial,
    })
}

pub(super) async fn versions(
    http: &Http,
    repository: &str,
    cancel: &CancellationToken,
) -> Result<Vec<Version>, Fault> {
    let url = http.repository(&["api", "models"], repository, &["refs"]);
    let (value, _) = http.json(url, cancel).await?;
    let mut versions = Vec::new();
    for field in ["branches", "tags"] {
        let rows = value
            .get(field)
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Fault::invalid("Hugging Face version listing is invalid"))?;
        for row in rows {
            let name = row.get("name").and_then(serde_json::Value::as_str);
            let revision = row.get("targetCommit").and_then(serde_json::Value::as_str);
            if let (Some(name), Some(revision)) = (name, revision)
                && commit(revision)
                && !name.is_empty()
                && name.len() <= 256
                && !name.contains('\0')
            {
                versions.push(Version {
                    name: name.into(),
                    revision: revision.to_ascii_lowercase(),
                });
            } else {
                return Err(Fault::invalid("Hugging Face version is not pinned"));
            }
        }
    }
    Ok(versions)
}

pub(super) async fn repository(
    http: &Http,
    source: &Source,
    cancel: &CancellationToken,
) -> Result<Repository, Fault> {
    let mut url = http.repository(
        &["api", "models"],
        &source.repository,
        &["revision", &source.revision],
    );
    url.query_pairs_mut().append_pair("blobs", "true");
    let (value, _) = http.json(url, cancel).await?;
    let revision = value
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .filter(|revision| commit(revision))
        .ok_or_else(|| Fault::invalid("Hugging Face model revision is not pinned"))?;
    if commit(&source.revision) && !source.revision.eq_ignore_ascii_case(revision) {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "Hugging Face returned a different model revision",
        ));
    }
    if value.get("private").and_then(serde_json::Value::as_bool) == Some(true)
        || value.get("gated").is_some_and(|value| {
            value.as_bool() == Some(true) || value.as_str().is_some_and(|value| value != "false")
        })
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "model repository requires authentication",
        ));
    }
    let pinned = Source {
        revision: revision.to_ascii_lowercase(),
        ..source.clone()
    };
    let rows = value
        .get("siblings")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Fault::invalid("Hugging Face file listing is invalid"))?;
    let mut files = BTreeMap::new();
    for row in rows {
        let path = row
            .get("rfilename")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Fault::invalid("Hugging Face file path is invalid"))?;
        let bytes = row
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| Fault::invalid("Hugging Face file size is unavailable"))?;
        let sha256 = row
            .pointer("/lfs/sha256")
            .and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase);
        if sha256.is_some()
            && row.pointer("/lfs/size").and_then(serde_json::Value::as_u64) != Some(bytes)
        {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "Hugging Face file sizes differ in metadata",
            ));
        }
        let blob = row
            .get("blobId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase);
        if sha256.as_deref().is_some_and(|value| !digest(value))
            || sha256.is_none() && !blob.as_deref().is_some_and(commit)
        {
            return Err(Fault::invalid("Hugging Face file checksum is unavailable"));
        }
        if files
            .insert(
                path.into(),
                File {
                    source: pinned.clone(),
                    path: path.into(),
                    bytes,
                    sha256,
                    blob,
                },
            )
            .is_some()
        {
            return Err(Fault::invalid(
                "Hugging Face file listing contains duplicates",
            ));
        }
    }
    Ok(Repository {
        source: pinned,
        name: source.repository.clone(),
        license: value
            .pointer("/cardData/license")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        files,
    })
}
