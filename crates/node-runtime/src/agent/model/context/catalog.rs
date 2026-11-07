//! Context lookup and the context-only limits.json projection derive from Goose
//! 14741e9a canonical/{name_builder.rs,data/canonical_models.json} (Apache-2.0).
//! Only providers reachable through Sailry's existing API families are retained.
//! See third_party_licenses/goose-Apache-2.0.txt.
use regex::Regex;
use sailry_protocol::conversation::ModelApi;
use std::{collections::BTreeMap, sync::OnceLock};

const DEFAULT_LIMIT: usize = 128_000;

pub(in crate::agent::model) fn limit(api: ModelApi, model: &str) -> usize {
    let provider = match api {
        ModelApi::Responses | ModelApi::ChatCompletions => "openai",
        ModelApi::Anthropic => "anthropic",
        ModelApi::Gemini => "google",
        ModelApi::AzureOpenAi | ModelApi::AzureAi => "azure",
        ModelApi::DeepSeek => "custom_deepseek",
        ModelApi::OpenCodeGo => "opencode-go",
        ModelApi::OpenCodeZen => "opencode",
        ModelApi::Plugin => "sailry_plugin",
    };
    let meta = api == ModelApi::AzureAi;
    let resolve = |name: &str| lookup(provider, name, meta);
    if !meta {
        return resolve(model).unwrap_or(DEFAULT_LIMIT);
    }
    resolve(model)
        .or_else(|| resolve(&model.to_ascii_lowercase()))
        .or_else(|| {
            static REASONING: OnceLock<Regex> = OnceLock::new();
            let reasoning = REASONING.get_or_init(|| {
                Regex::new(r"(?i)(?:^|[-/])(?:o\d+(?:$|-)|gpt-(?:5|6)(?:$|[-.]))")
                    .expect("static reasoning model pattern")
            });
            let (base, effort) = model.rsplit_once('-')?;
            (reasoning.is_match(model)
                && matches!(
                    effort.to_ascii_lowercase().as_str(),
                    "none" | "low" | "medium" | "high" | "xhigh"
                ))
            .then(|| resolve(base).or_else(|| resolve(&base.to_ascii_lowercase())))
            .flatten()
        })
        .unwrap_or(DEFAULT_LIMIT)
}

fn get(provider: &str, model: &str) -> Option<usize> {
    type Limits = BTreeMap<String, BTreeMap<String, usize>>;
    static LIMITS: OnceLock<Limits> = OnceLock::new();
    let limits = LIMITS.get_or_init(|| {
        let grouped: BTreeMap<String, BTreeMap<usize, Vec<String>>> =
            serde_json::from_str(include_str!("limits.json"))
                .expect("bundled context limits are valid");
        grouped
            .into_iter()
            .map(|(provider, groups)| {
                let models = groups
                    .into_iter()
                    .flat_map(|(limit, models)| models.into_iter().map(move |model| (model, limit)))
                    .collect();
                (provider, models)
            })
            .collect()
    });
    limits.get(provider)?.get(model).copied()
}

fn lookup(provider: &str, model: &str, meta: bool) -> Option<usize> {
    let normalized = normalize(model);
    if !meta && let Some(limit) = get(provider, &normalized).or_else(|| get(provider, model)) {
        return Some(limit);
    }
    let stripped = strip_prefix(model);
    let inferred = |name: &str| get(infer(name)?, &normalize(name));
    if let Some(limit) = swap_claude(stripped).as_deref().and_then(inferred) {
        return Some(limit);
    }
    if let Some(limit) = inferred(stripped).or_else(|| inferred(model)) {
        return Some(limit);
    }
    for prefix in [
        "anthropic",
        "openai",
        "google",
        "meta-llama",
        "mistralai",
        "cohere",
        "ai21",
        "amazon",
        "deepseek",
        "qwen",
        "x-ai",
        "nvidia",
        "microsoft",
        "perplexity",
    ] {
        if let Some(name) = stripped.strip_prefix(&format!("{prefix}-"))
            && let Some(limit) = get(prefix, &normalize(name))
        {
            return Some(limit);
        }
    }
    meta.then(|| {
        get(provider, model)
            .or_else(|| get(provider, &normalized))
            .or_else(|| get(provider, &normalized.to_ascii_lowercase()))
    })
    .flatten()
}

fn normalize(model: &str) -> String {
    static VERSION: OnceLock<Regex> = OnceLock::new();
    static SUFFIXES: OnceLock<Vec<Regex>> = OnceLock::new();
    let version = VERSION
        .get_or_init(|| Regex::new(r"-(\d)-(\d)(-|@|$)").expect("static model version pattern"));
    let suffixes = SUFFIXES.get_or_init(|| {
        [
            r"-latest$",
            r"-\d{8}$",
            r"@\d{8}$",
            r"-\d{4}$",
            r"-\d{4}-\d{2}-\d{2}$",
            r"-bedrock$",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("static model suffix pattern"))
        .collect()
    });
    let mut value = version.replace_all(model, "-$1.$2$3").to_string();
    loop {
        let before = value.clone();
        for suffix in suffixes {
            value = suffix.replace(&value, "").to_string();
        }
        if value == before {
            return value;
        }
    }
}

fn strip_prefix(model: &str) -> &str {
    let lowercase = model.to_lowercase();
    [
        "claude-",
        "gpt-",
        "gemini-",
        "gemma-",
        "o1-",
        "o1",
        "o3-",
        "o3",
        "o4-",
        "llama-",
        "mistral-",
        "mixtral-",
        "chatgpt-",
        "deepseek-",
        "qwen-",
        "grok-",
        "jamba-",
        "command-",
        "codestral",
        "ministral-",
        "pixtral-",
        "devstral-",
        "glm-",
    ]
    .into_iter()
    .filter_map(|pattern| lowercase.find(pattern))
    .min()
    .and_then(|index| model.get(index..))
    .unwrap_or(model)
}

fn swap_claude(model: &str) -> Option<String> {
    if !model.starts_with("claude-") {
        return None;
    }
    static PATTERNS: OnceLock<Vec<(Regex, Regex, &'static str)>> = OnceLock::new();
    for (forward, reverse, size) in PATTERNS.get_or_init(|| {
        ["sonnet", "opus", "haiku", "fable"]
            .into_iter()
            .map(|size| {
                (
                    Regex::new(&format!("claude-([0-9.-]+)-{size}"))
                        .expect("static Claude model pattern"),
                    Regex::new(&format!("claude-{size}-([0-9.-]+)"))
                        .expect("static Claude model pattern"),
                    size,
                )
            })
            .collect()
    }) {
        if let Some(captures) = forward.captures(model) {
            return Some(format!("claude-{size}-{}", &captures[1]));
        }
        if let Some(captures) = reverse.captures(model) {
            return Some(format!("claude-{}-{size}", &captures[1]));
        }
    }
    None
}

fn infer(model: &str) -> Option<&'static str> {
    let model = model.to_lowercase();
    for (provider, matches) in [
        ("anthropic", model.contains("claude")),
        (
            "openai",
            ["gpt-", "o1", "o3", "o4", "chatgpt-"]
                .iter()
                .any(|prefix| model.starts_with(prefix)),
        ),
        (
            "google",
            ["gemini-", "gemma-"]
                .iter()
                .any(|prefix| model.starts_with(prefix)),
        ),
        ("meta-llama", model.contains("llama")),
        (
            "mistralai",
            [
                "mistral",
                "mixtral",
                "codestral",
                "ministral",
                "pixtral",
                "devstral",
                "voxtral",
            ]
            .iter()
            .any(|prefix| model.starts_with(prefix)),
        ),
        ("deepseek", model.contains("deepseek")),
        ("qwen", model.contains("qwen")),
        ("x-ai", model.contains("grok")),
        ("ai21", model.contains("jamba")),
        ("cohere", model.contains("command")),
        ("zhipuai", model.starts_with("glm-")),
    ] {
        if matches {
            return Some(provider);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_captured_limits_and_defaults() {
        for (api, model, expected) in [
            (ModelApi::Responses, "gpt-6.1-sol", 1_050_000),
            (ModelApi::Responses, "gpt-4o-latest", 128_000),
            (ModelApi::Responses, "gpt-5-codex", 128_000),
            (ModelApi::Anthropic, "claude-sonnet-4-5-20250929", 200_000),
            (ModelApi::Anthropic, "claude-4-5-sonnet", 200_000),
            (ModelApi::Gemini, "gemini-2.5-pro", 1_048_576),
            (ModelApi::AzureAi, "GPT-6.1-SOL-HIGH", 1_050_000),
            (ModelApi::AzureAi, "Phi-4", 128_000),
            (ModelApi::AzureAi, "deepseek-v4-pro", 1_048_576),
            (ModelApi::AzureOpenAi, "deepseek-v4-pro", 1_000_000),
            (ModelApi::DeepSeek, "deepseek-v4-pro", 1_048_576),
            (ModelApi::OpenCodeGo, "kimi-k3", 1_048_576),
            (ModelApi::OpenCodeZen, "claude-sonnet-4-5", 1_000_000),
            (ModelApi::Plugin, "hosted-claude-sonnet-4-5", 200_000),
            (ModelApi::Plugin, "unknown-model", 128_000),
        ] {
            assert_eq!(limit(api, model), expected, "{api:?}/{model}");
        }
    }
}
