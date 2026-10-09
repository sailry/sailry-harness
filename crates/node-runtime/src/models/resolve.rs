use super::{File, Repository, SMALL_BYTES, http::Http, huggingface, modelscope};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Asset, Definition, Format, Hub, Kind, MAX_ASSET_BYTES, Pooling, Source},
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) mod ocr;
mod prefixes;

pub(super) async fn definition(
    http: &Http,
    kind: Kind,
    source: &Source,
    cancel: &CancellationToken,
) -> Result<Definition, Fault> {
    let repository = repository(http, source, cancel).await?;
    let (query_prefix, document_prefix) = match kind {
        Kind::Embedding => prefixes::read(http, &repository, cancel).await?,
        Kind::Speech | Kind::Ocr => (None, None),
    };
    let has_prefix = [&query_prefix, &document_prefix]
        .into_iter()
        .flatten()
        .any(|value| !value.is_empty());
    let (format, assets) = match kind {
        Kind::Embedding => embedding(http, &repository, has_prefix, cancel).await?,
        Kind::Speech => speech(http, &repository, cancel).await?,
        Kind::Ocr => ocr::definition(http, &repository, cancel).await?,
    };
    let definition = Definition {
        source: repository.source,
        name: repository.name,
        license: repository.license,
        format,
        query_prefix,
        document_prefix,
        assets,
    };
    definition.validate()?;
    Ok(definition)
}

async fn repository(
    http: &Http,
    source: &Source,
    cancel: &CancellationToken,
) -> Result<Repository, Fault> {
    source.validate()?;
    let source = if source.revision == "HEAD" {
        match source.hub {
            Hub::HuggingFace => Source {
                revision: "main".into(),
                ..source.clone()
            },
            Hub::ModelScope => modelscope::head(http, &source.repository, cancel).await?,
        }
    } else {
        source.clone()
    };
    match source.hub {
        Hub::HuggingFace => huggingface::repository(http, &source, cancel).await,
        Hub::ModelScope => modelscope::repository(http, &source, cancel).await,
    }
}

async fn embedding(
    http: &Http,
    repository: &Repository,
    has_prefix: bool,
    cancel: &CancellationToken,
) -> Result<(Format, Vec<Asset>), Fault> {
    let (format, config_asset) = embedding_profile(http, repository, has_prefix, cancel).await?;
    let tokenizer = small(
        http,
        required(repository, "tokenizer.json")?,
        "tokenizer.json",
        cancel,
    )
    .await?
    .1;
    let file = match format {
        Format::Static { .. } => required(repository, "model.safetensors")?,
        Format::Transformer { .. } => graph(repository, false)?,
        Format::SenseVoice {} | Format::PaddleOcr { .. } => unreachable!(),
    };
    let name = if matches!(format, Format::Static { .. }) {
        "model.safetensors"
    } else {
        "model.onnx"
    };
    let weights = asset(http, file, name, cancel).await?;
    Ok((format, vec![config_asset, tokenizer, weights]))
}

async fn embedding_profile(
    http: &Http,
    repository: &Repository,
    has_prefix: bool,
    cancel: &CancellationToken,
) -> Result<(Format, Asset), Fault> {
    let config = required(repository, "config.json")?;
    let (value, config_asset) = json(http, config, "config.json", cancel).await?;
    let format = if value.get("model_type").and_then(Value::as_str) == Some("model2vec") {
        if value.get("normalize").and_then(Value::as_bool) != Some(true) {
            return Err(Fault::invalid(
                "static model requires normalized mean pooling",
            ));
        }
        Format::Static {
            dimension: dimension(&value, "hidden_dim")?,
        }
    } else {
        let dimension = dimension(&value, "hidden_size")?;
        let pooling = pooling(http, repository, &value, dimension, has_prefix, cancel).await?;
        let token_types = match value.get("type_vocab_size") {
            None => false,
            Some(value) => {
                value
                    .as_u64()
                    .ok_or_else(|| Fault::invalid("model token type configuration is invalid"))?
                    > 0
            }
        };
        Format::Transformer {
            dimension,
            pooling,
            token_types,
        }
    };
    Ok((format, config_asset))
}

async fn speech(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(Format, Vec<Asset>), Fault> {
    speech_profile(http, repository, cancel).await?;
    // Repository metadata identifies a candidate. Installation still verifies
    // the actual single-file graph and constructs the SenseVoice recognizer.
    let graph = asset(http, graph(repository, true)?, "model.onnx", cancel).await?;
    let tokens = small(
        http,
        required(repository, "tokens.txt")?,
        "tokens.txt",
        cancel,
    )
    .await?
    .1;
    Ok((Format::SenseVoice {}, vec![graph, tokens]))
}

async fn speech_profile(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let mut sense_voice = repository
        .source
        .repository
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
        .contains("sensevoice");
    if let Some(config) = repository.files.get("config.json") {
        let (value, _) = json(http, config, "config.json", cancel).await?;
        sense_voice = value
            .get("model_type")
            .and_then(Value::as_str)
            .is_some_and(|value| matches!(value, "sense_voice" | "sensevoice" | "SenseVoice"));
    }
    if !sense_voice {
        return Err(Fault::invalid(
            "only SenseVoice speech models are supported",
        ));
    }
    Ok(())
}

pub(super) async fn inspect(
    http: &Http,
    kind: Kind,
    hub: Hub,
    candidate: super::Candidate,
    cancel: &CancellationToken,
) -> Result<sailry_protocol::models::Item, Fault> {
    let source = match hub {
        Hub::HuggingFace => Source {
            hub,
            repository: candidate.repository,
            revision: "main".into(),
        },
        Hub::ModelScope => modelscope::head(http, &candidate.repository, cancel).await?,
    };
    let repository = match hub {
        Hub::HuggingFace => huggingface::repository(http, &source, cancel).await?,
        Hub::ModelScope => modelscope::repository(http, &source, cancel).await?,
    };
    if !super::commit(&repository.source.revision) && kind != Kind::Ocr {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "model catalog could not pin the repository revision",
        ));
    }
    // Listing inspection never downloads tokenizer, token tables or model weights.
    for file in repository.files.values().filter(|file| {
        file.path.ends_with("config.json")
            || matches!(
                file.path.as_str(),
                "modules.json" | "config_sentence_transformers.json"
            )
    }) {
        if file.bytes > super::METADATA_BYTES as u64 {
            return Err(Fault::invalid(
                "model configuration exceeds its metadata limit",
            ));
        }
    }
    let format = match kind {
        Kind::Embedding => {
            let (query, document) = prefixes::read(http, &repository, cancel).await?;
            let has_prefix = [query, document]
                .into_iter()
                .flatten()
                .any(|value| !value.is_empty());
            let (format, _) = embedding_profile(http, &repository, has_prefix, cancel).await?;
            let tokenizer = required(&repository, "tokenizer.json")?;
            if tokenizer.bytes == 0 || tokenizer.bytes > SMALL_BYTES as u64 {
                return Err(Fault::invalid("model tokenizer exceeds its size limit"));
            }
            let weights = match format {
                Format::Static { .. } => required(&repository, "model.safetensors")?,
                Format::Transformer { .. } => graph(&repository, false)?,
                Format::SenseVoice {} | Format::PaddleOcr { .. } => unreachable!(),
            };
            if weights.bytes == 0 || weights.bytes > MAX_ASSET_BYTES {
                return Err(Fault::invalid("model asset exceeds its size limit"));
            }
            format
        }
        Kind::Speech => {
            speech_profile(http, &repository, cancel).await?;
            let tokens = required(&repository, "tokens.txt")?;
            let weights = graph(&repository, true)?;
            if tokens.bytes == 0
                || tokens.bytes > SMALL_BYTES as u64
                || weights.bytes == 0
                || weights.bytes > MAX_ASSET_BYTES
            {
                return Err(Fault::invalid("model asset exceeds its size limit"));
            }
            Format::SenseVoice {}
        }
        Kind::Ocr => ocr::profile(http, &repository, cancel).await?.0,
    };
    Ok(sailry_protocol::models::Item {
        source: repository.source,
        name: candidate.name,
        format,
    })
}

async fn pooling(
    http: &Http,
    repository: &Repository,
    config: &Value,
    dimension: u16,
    has_prefix: bool,
    cancel: &CancellationToken,
) -> Result<Pooling, Fault> {
    let mut path = repository
        .files
        .contains_key("1_Pooling/config.json")
        .then(|| "1_Pooling/config.json".to_owned());
    if let Some(modules) = repository.files.get("modules.json") {
        let (modules, _) = json(http, modules, "metadata.json", cancel).await?;
        let modules = modules
            .as_array()
            .ok_or_else(|| Fault::invalid("model module configuration is invalid"))?;
        let mut transformer = false;
        let mut seen = false;
        let mut normalized = false;
        for module in modules {
            match module.get("type").and_then(Value::as_str) {
                Some("sentence_transformers.models.Transformer")
                    if !transformer
                        && !seen
                        && module.get("path").and_then(Value::as_str) == Some("") =>
                {
                    transformer = true;
                }
                Some("sentence_transformers.models.Normalize") if seen && !normalized => {
                    normalized = true;
                }
                Some("sentence_transformers.models.Pooling") if transformer && !seen => {
                    let directory = module
                        .get("path")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| Fault::invalid("model pooling path is invalid"))?;
                    path = Some(format!("{directory}/config.json"));
                    seen = true;
                }
                _ => {
                    return Err(Fault::invalid(
                        "model requires an unsupported embedding module",
                    ));
                }
            }
        }
        if !transformer || !seen {
            return Err(Fault::invalid(
                "model embedding module sequence is incomplete",
            ));
        }
    }
    if let Some(path) = path {
        let file = required(repository, &path)?;
        let (value, _) = json(http, file, "metadata.json", cancel).await?;
        if has_prefix
            && value
                .get("pooling_mode_mean_tokens")
                .and_then(Value::as_bool)
                == Some(true)
            && value
                .get("include_prompt")
                .is_some_and(|value| value.as_bool() != Some(true))
        {
            return Err(Fault::invalid(
                "model requires excluding prompt tokens from pooling",
            ));
        }
        if value
            .get("word_embedding_dimension")
            .and_then(Value::as_u64)
            != Some(u64::from(dimension))
        {
            return Err(Fault::invalid(
                "model pooling dimension differs from its configuration",
            ));
        }
        for field in [
            "pooling_mode_max_tokens",
            "pooling_mode_mean_sqrt_len_tokens",
            "pooling_mode_weightedmean_tokens",
            "pooling_mode_lasttoken",
        ] {
            if value
                .get(field)
                .is_some_and(|value| value.as_bool() != Some(false))
            {
                return Err(Fault::invalid(
                    "model requires an unsupported pooling operation",
                ));
            }
        }
        return match (
            value.get("pooling_mode_cls_token").and_then(Value::as_bool),
            value
                .get("pooling_mode_mean_tokens")
                .and_then(Value::as_bool),
        ) {
            (Some(true), Some(false)) => Ok(Pooling::Cls),
            (Some(false), Some(true)) => Ok(Pooling::Mean),
            _ => Err(Fault::invalid("model pooling operation is ambiguous")),
        };
    }
    match config.get("classifier_pooling").and_then(Value::as_str) {
        Some("cls") => Ok(Pooling::Cls),
        Some("mean") => Ok(Pooling::Mean),
        _ => Err(Fault::invalid("model pooling metadata is unavailable")),
    }
}

fn graph(repository: &Repository, speech: bool) -> Result<&File, Fault> {
    let names: &[&str] = if speech {
        &[
            "model.int8.onnx",
            "onnx/model.int8.onnx",
            "model.onnx",
            "onnx/model.onnx",
        ]
    } else {
        &[
            "model.onnx",
            "onnx/model.onnx",
            "model.int8.onnx",
            "onnx/model.int8.onnx",
            "onnx/model_quantized.onnx",
        ]
    };
    names
        .iter()
        .find_map(|name| repository.files.get(*name))
        .ok_or_else(|| Fault::invalid("model repository has no supported single-file ONNX graph"))
}

fn required<'a>(repository: &'a Repository, path: &str) -> Result<&'a File, Fault> {
    repository
        .files
        .get(path)
        .ok_or_else(|| Fault::invalid(format!("model asset is missing: {path}")))
}

fn dimension(value: &Value, key: &str) -> Result<u16, Fault> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (1..=4096).contains(value))
        .ok_or_else(|| Fault::invalid("model embedding dimension is unsupported"))
}

async fn json(
    http: &Http,
    file: &File,
    name: &str,
    cancel: &CancellationToken,
) -> Result<(Value, Asset), Fault> {
    let (body, asset) = small(http, file, name, cancel).await?;
    let value = serde_json::from_slice(&body)
        .map_err(|_| Fault::invalid(format!("model asset contains invalid JSON: {}", file.path)))?;
    Ok((value, asset))
}

async fn asset(
    http: &Http,
    file: &File,
    name: &str,
    cancel: &CancellationToken,
) -> Result<Asset, Fault> {
    if let Some(checksum) = &file.sha256 {
        if file.bytes == 0 || file.bytes > MAX_ASSET_BYTES {
            return Err(Fault::invalid("model asset exceeds its size limit"));
        }
        return Ok(pinned(file, name, checksum.clone()));
    }
    Ok(small(http, file, name, cancel).await?.1)
}

async fn small(
    http: &Http,
    file: &File,
    name: &str,
    cancel: &CancellationToken,
) -> Result<(Vec<u8>, Asset), Fault> {
    if file.bytes == 0 || file.bytes > SMALL_BYTES as u64 {
        return Err(Fault::invalid(format!(
            "model metadata asset exceeds its size limit: {}",
            file.path
        )));
    }
    let response = http.get(http.file(file)?, SMALL_BYTES, cancel).await?;
    let body = response.body;
    if body.len() as u64 != file.bytes {
        return Err(changed(file));
    }
    let checksum = format!("{:x}", Sha256::digest(&body));
    if let Some(expected) = &file.sha256 {
        if !checksum.eq_ignore_ascii_case(expected) {
            return Err(changed(file));
        }
    } else {
        let mut hash = sha1::Sha1::new();
        hash.update(format!("blob {}\0", body.len()));
        hash.update(&body);
        let checksum = format!("{:x}", hash.finalize());
        if !file
            .blob
            .as_deref()
            .is_some_and(|expected| checksum.eq_ignore_ascii_case(expected))
        {
            return Err(changed(file));
        }
    }
    let asset = pinned(file, name, checksum);
    Ok((body, asset))
}

fn pinned(file: &File, name: &str, sha256: String) -> Asset {
    Asset {
        source: file.source.clone(),
        path: file.path.clone(),
        name: name.into(),
        bytes: file.bytes,
        sha256,
    }
}

fn changed(file: &File) -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        format!(
            "model file metadata differs at the selected revision: {}",
            file.path
        ),
    )
}
