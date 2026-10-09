use super::*;
use sailry_protocol::models::MAX_PREFIX_BYTES;

pub(super) async fn read(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(Option<String>, Option<String>), Fault> {
    let Some(file) = repository.files.get("config_sentence_transformers.json") else {
        return Ok((None, None));
    };
    let (value, _) = json(http, file, "metadata.json", cancel).await?;
    let value = value.as_object().ok_or_else(invalid)?;
    let prompts = match value.get("prompts") {
        None => None,
        Some(value) => Some(value.as_object().ok_or_else(invalid)?),
    };
    let prompt = |name: &str| -> Result<Option<String>, Fault> {
        prompts
            .and_then(|prompts| prompts.get(name))
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| value.len() <= MAX_PREFIX_BYTES && !value.contains('\0'))
                    .map(str::to_owned)
                    .ok_or_else(invalid)
            })
            .transpose()
    };
    let default = match value.get("default_prompt_name") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let name = value
                .as_str()
                .filter(|value| matches!(*value, "query" | "document" | "passage" | "corpus"))
                .ok_or_else(|| Fault::invalid("model requires an unsupported default prompt"))?;
            Some(prompt(name)?.ok_or_else(invalid)?)
        }
    };
    let query = prompt("query")?.or_else(|| default.clone());
    let document = prompt("document")?
        .or(prompt("passage")?)
        .or(prompt("corpus")?)
        .or(default);
    Ok((query, document))
}

fn invalid() -> Fault {
    Fault::invalid("model input prompt metadata is invalid")
}
