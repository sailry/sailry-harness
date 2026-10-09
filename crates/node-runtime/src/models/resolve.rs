use super::{File, Repository, SMALL_BYTES, http::Http, huggingface, modelscope};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Asset, Definition, Format, Hub, Kind, MAX_ASSET_BYTES, Pooling, Source},
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) async fn definition(
    http: &Http,
    kind: Kind,
    source: &Source,
    cancel: &CancellationToken,
) -> Result<Definition, Fault> {
    source.validate()?;
    let repository = match source.hub {
        Hub::HuggingFace => huggingface::repository(http, source, cancel).await?,
        Hub::ModelScope => modelscope::repository(http, source, cancel).await?,
    };
    let (format, assets) = match kind {
        Kind::Embedding => embedding(http, &repository, cancel).await?,
        Kind::Speech => speech(http, &repository, cancel).await?,
    };
    let definition = Definition {
        source: repository.source,
        name: repository.name,
        license: repository.license,
        format,
        assets,
    };
    definition.validate()?;
    Ok(definition)
}

async fn embedding(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(Format, Vec<Asset>), Fault> {
    let config = required(repository, "config.json")?;
    let (value, config_asset) = json(http, config, "config.json", cancel).await?;
    let tokenizer = required(repository, "tokenizer.json")?;
    let tokenizer = small(http, tokenizer, "tokenizer.json", cancel).await?.1;
    if value.get("model_type").and_then(Value::as_str) == Some("model2vec") {
        if value.get("normalize").and_then(Value::as_bool) != Some(true) {
            return Err(Fault::invalid(
                "static model requires normalized mean pooling",
            ));
        }
        let dimension = dimension(&value, "hidden_dim")?;
        let table = asset(
            http,
            required(repository, "model.safetensors")?,
            "model.safetensors",
            cancel,
        )
        .await?;
        return Ok((
            Format::Static { dimension },
            vec![config_asset, tokenizer, table],
        ));
    }
    let dimension = dimension(&value, "hidden_size")?;
    let pooling = pooling(http, repository, &value, dimension, cancel).await?;
    let token_types = match value.get("type_vocab_size") {
        None => false,
        Some(value) => {
            value
                .as_u64()
                .ok_or_else(|| Fault::invalid("model token type configuration is invalid"))?
                > 0
        }
    };
    let graph = graph(repository, false)?;
    let graph = asset(http, graph, "model.onnx", cancel).await?;
    Ok((
        Format::Transformer {
            dimension,
            pooling,
            token_types,
        },
        vec![config_asset, tokenizer, graph],
    ))
}

async fn speech(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(Format, Vec<Asset>), Fault> {
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

async fn pooling(
    http: &Http,
    repository: &Repository,
    config: &Value,
    dimension: u16,
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
