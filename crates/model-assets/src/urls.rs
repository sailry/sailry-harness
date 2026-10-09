use crate::{Error, Result};
use reqwest::Url;
use sailry_protocol::models::{Asset, Hub};

pub fn url(asset: &Asset, mirror: Option<&str>) -> Result<Url> {
    asset.source.validate().map_err(|_| Error::InvalidAsset)?;
    if asset.source.revision.len() != 40
        || !asset
            .source
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || asset.path.is_empty()
        || asset.path.split('/').any(|part| {
            matches!(part, "" | "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(Error::InvalidAsset);
    }
    let base = match asset.source.hub {
        Hub::HuggingFace => mirror.unwrap_or("https://huggingface.co"),
        Hub::ModelScope => "https://modelscope.cn/api/v1/models",
    };
    let mut url = Url::parse(base).map_err(|_| Error::InvalidAsset)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidAsset);
    }
    {
        let mut path = url.path_segments_mut().map_err(|_| Error::InvalidAsset)?;
        path.pop_if_empty();
        path.extend(asset.source.repository.split('/'));
        match asset.source.hub {
            Hub::HuggingFace => {
                path.extend(["resolve", &asset.source.revision]);
                path.extend(asset.path.split('/'));
            }
            Hub::ModelScope => {
                path.push("repo");
            }
        }
    }
    if asset.source.hub == Hub::ModelScope {
        url.query_pairs_mut()
            .append_pair("Revision", &asset.source.revision)
            .append_pair("FilePath", &asset.path);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::models::Source;

    fn asset(hub: Hub) -> Asset {
        Asset {
            source: Source {
                hub,
                repository: "org/model".into(),
                revision: "a".repeat(40),
            },
            path: "onnx/model.int8.onnx".into(),
            name: "model.onnx".into(),
            bytes: 5,
            sha256: "1".repeat(64),
        }
    }

    #[test]
    fn preserves_remote_paths_and_mirrors() {
        assert_eq!(
            url(
                &asset(Hub::HuggingFace),
                Some("https://mirror.example/cache/")
            )
            .unwrap()
            .as_str(),
            format!(
                "https://mirror.example/cache/org/model/resolve/{}/onnx/model.int8.onnx",
                "a".repeat(40)
            )
        );
        let url = url(&asset(Hub::ModelScope), Some("https://ignored.example")).unwrap();
        assert_eq!(url.host_str(), Some("modelscope.cn"));
        assert_eq!(url.path(), "/api/v1/models/org/model/repo");
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![
                ("Revision".into(), "a".repeat(40).into()),
                ("FilePath".into(), "onnx/model.int8.onnx".into())
            ]
        );
    }

    #[test]
    fn rejects_unpinned_and_escaping_assets() {
        for path in ["../model.onnx", "/model.onnx", "onnx\\model.onnx", ""] {
            let mut asset = asset(Hub::HuggingFace);
            asset.path = path.into();
            assert_eq!(url(&asset, None), Err(Error::InvalidAsset));
        }
        let mut asset = asset(Hub::HuggingFace);
        asset.source.revision = "main".into();
        assert_eq!(url(&asset, None), Err(Error::InvalidAsset));
    }
}
