//! Decision discovery selects one immutable, root-level GGUF without reading its header.
use super::*;

pub(super) async fn definition(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<Vec<Asset>, Fault> {
    Ok(vec![
        asset(http, select(&repository.files)?, "model.gguf", cancel).await?,
    ])
}

pub(super) fn select(files: &std::collections::BTreeMap<String, File>) -> Result<&File, Fault> {
    let candidates = files
        .values()
        .filter(|file| {
            let name = file.path.to_ascii_lowercase();
            !name.contains('/')
                && name.ends_with(".gguf")
                && !sharded(&name)
                && file.bytes > 0
                && file.bytes <= MAX_ASSET_BYTES
        })
        .collect::<Vec<_>>();
    for quantization in ["q8_0", "q4_k_m"] {
        let selected = candidates
            .iter()
            .filter(|file| file.path.to_ascii_lowercase().contains(quantization))
            .copied()
            .collect::<Vec<_>>();
        match selected.as_slice() {
            [file] => return Ok(*file),
            [] => {}
            _ => return Err(Fault::invalid("decision model quantization is ambiguous")),
        }
    }
    match candidates.as_slice() {
        [file] => Ok(*file),
        [] => Err(Fault::invalid("repository has no supported decision model")),
        _ => Err(Fault::invalid("decision model file is ambiguous")),
    }
}

fn sharded(name: &str) -> bool {
    let stem = name.strip_suffix(".gguf").unwrap_or(name);
    let Some((prefix, count)) = stem.rsplit_once("-of-") else {
        return false;
    };
    let Some((_, index)) = prefix.rsplit_once('-') else {
        return false;
    };
    !index.is_empty()
        && !count.is_empty()
        && index.bytes().all(|byte| byte.is_ascii_digit())
        && count.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn files(names: &[&str]) -> BTreeMap<String, File> {
        names
            .iter()
            .map(|name| {
                (
                    (*name).into(),
                    File {
                        source: Source {
                            hub: Hub::HuggingFace,
                            repository: "owner/model".into(),
                            revision: "a".repeat(40),
                        },
                        path: (*name).into(),
                        bytes: 100,
                        sha256: Some("b".repeat(64)),
                        blob: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn prefers_fixed_quantizations_and_ordinary_files() {
        let files = files(&[
            "model-Q4_K_M.gguf",
            "model-Q8_0.gguf",
            "subdir/other-Q8_0.gguf",
            "model-Q8_0-00001-of-00002.gguf",
        ]);
        assert_eq!(select(&files).unwrap().path, "model-Q8_0.gguf");
        let files = self::files(&["model-F16.gguf"]);
        assert_eq!(select(&files).unwrap().path, "model-F16.gguf");
    }

    #[test]
    fn rejects_ambiguous_or_unsupported_exports() {
        for names in [
            &["first-Q8_0.gguf", "second-Q8_0.gguf"][..],
            &["first.gguf", "second.gguf"][..],
            &["subdir/model.gguf"][..],
            &["model-00001-of-00002.gguf"][..],
        ] {
            assert!(select(&files(names)).is_err());
        }
        let mut files = files(&["model.gguf"]);
        files.get_mut("model.gguf").unwrap().bytes = MAX_ASSET_BYTES + 1;
        assert!(select(&files).is_err());
    }
}
