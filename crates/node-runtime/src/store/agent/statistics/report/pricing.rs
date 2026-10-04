//! Reference estimates follow Code 67ae9fa0 pricing.rs and the current models.dev cost schema.
//! Read only the existing catalog and canonical events; use exact model IDs for reference prices.
use super::*;
use adk_core::UsageMetadata;
use sailry_protocol::usage::{Cost, CostBreakdown};
use serde_json::Value;

#[derive(Default)]
pub(super) struct Catalog {
    models: BTreeMap<(String, String), Option<Value>>,
}

impl Catalog {
    pub(super) fn estimate(
        &mut self,
        db: &Connection,
        row: &rusqlite::Row<'_>,
    ) -> Result<Option<Cost>, Fault> {
        let usage: String = row.get(12).map_err(storage_error)?;
        let usage: UsageMetadata = serde_json::from_str(&usage).map_err(storage_error)?;
        let reported = usage.cost.and_then(micros).map(|usd_micros| Cost {
            usd_micros,
            responses: 1,
            breakdown: None,
        });
        let media: Option<String> = row.get(14).map_err(storage_error)?;
        if matches!(media.as_deref(), Some("image" | "video")) {
            // Text token rates cannot price image modalities or video duration.
            return Ok(reported);
        }
        let api: Option<String> = row.get(9).map_err(storage_error)?;
        let authentication: Option<String> = row.get(10).map_err(storage_error)?;
        let endpoint: Option<String> = row.get(11).map_err(storage_error)?;
        let Some(supplier) = supplier(
            api.as_deref(),
            authentication.as_deref(),
            endpoint.as_deref(),
        ) else {
            return Ok(reported);
        };
        let model: String = row.get(6).map_err(storage_error)?;
        let key = (supplier.to_owned(), model);
        if !self.models.contains_key(&key) {
            let body: Option<Vec<u8>> = db
                .query_row(
                    "SELECT body FROM model_catalog WHERE provider=?1 AND id=?2",
                    params![key.0, key.1],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            let cost = body
                .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
                .and_then(|model| model.get("cost").cloned());
            self.models.insert(key.clone(), cost);
        }
        Ok(self.models[&key]
            .as_ref()
            .and_then(|cost| estimate(cost, &usage))
            .or(reported))
    }
}

fn supplier(
    api: Option<&str>,
    authentication: Option<&str>,
    endpoint: Option<&str>,
) -> Option<&'static str> {
    if authentication != Some("api_key") {
        return None;
    }
    // A custom endpoint can proxy the same model. These are catalog reference
    // prices, not a claim about that endpoint's billing. Missing exact IDs stay unpriced.
    endpoint?;
    match api? {
        "responses" | "chat_completions" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "gemini" => Some("google"),
        _ => None,
    }
}

fn micros(usd: f64) -> Option<u64> {
    let value = usd * 1_000_000.;
    (value.is_finite() && value >= 0. && value < u64::MAX as f64).then(|| value.round() as u64)
}

fn estimate(cost: &Value, usage: &UsageMetadata) -> Option<Cost> {
    let input = u64::try_from(usage.prompt_token_count).ok()?;
    let output = u64::try_from(usage.candidates_token_count).ok()?;
    let read = u64::try_from(usage.cache_read_input_token_count.unwrap_or(0)).ok()?;
    let write = u64::try_from(usage.cache_creation_input_token_count.unwrap_or(0)).ok()?;
    let reasoning = u64::try_from(usage.thinking_token_count.unwrap_or(0)).ok()?;
    let audio_input = u64::try_from(usage.audio_input_token_count.unwrap_or(0)).ok()?;
    let audio_output = u64::try_from(usage.audio_output_token_count.unwrap_or(0)).ok()?;
    // Audio and cached counts may overlap; the normalized aggregate cannot price that split.
    if audio_input > 0 && (read > 0 || write > 0) {
        return None;
    }
    let plain_input = input
        .checked_sub(read)?
        .checked_sub(write)?
        .checked_sub(audio_input)?;
    let plain_output = output.checked_sub(reasoning)?.checked_sub(audio_output)?;
    let cost = tier(cost, input)?;
    if (cost.get("input_audio").is_some() && usage.audio_input_token_count.is_none())
        || (cost.get("output_audio").is_some() && usage.audio_output_token_count.is_none())
    {
        return None;
    }
    micros(cost.get("input")?.as_f64()?)?;
    micros(cost.get("output")?.as_f64()?)?;
    let mut categories = [0u128; 4];
    for (count, field, fallback, category) in [
        (plain_input, "input", None, 0),
        (plain_output, "output", None, 1),
        (read, "cache_read", None, 2),
        (write, "cache_write", None, 3),
        (reasoning, "reasoning", Some("output"), 1),
        (audio_input, "input_audio", None, 0),
        (audio_output, "output_audio", None, 1),
    ] {
        if count == 0 {
            continue;
        }
        let value = cost
            .get(field)
            .or_else(|| fallback.and_then(|field| cost.get(field)))?;
        let rate = micros(value.as_f64()?)?;
        categories[category] =
            categories[category].checked_add(u128::from(rate).checked_mul(u128::from(count))?)?;
    }
    // Round cumulative charges so the disjoint amounts sum to the existing rounded total.
    let mut cumulative = 0u128;
    let mut total = 0u64;
    let mut amounts = [0u64; 4];
    for (index, category) in categories.into_iter().enumerate() {
        cumulative = cumulative.checked_add(category)?;
        let rounded = u64::try_from(cumulative.checked_add(500_000)? / 1_000_000).ok()?;
        amounts[index] = rounded - total;
        total = rounded;
    }
    Some(Cost {
        usd_micros: total,
        responses: 1,
        breakdown: Some(CostBreakdown {
            input: amounts[0],
            output: amounts[1],
            cache_read: amounts[2],
            cache_write: amounts[3],
        }),
    })
}

fn tier(cost: &Value, input: u64) -> Option<&Value> {
    let mut selected = cost;
    let mut threshold = 0;
    if let Some(tiers) = cost.get("tiers") {
        let mut sizes = BTreeSet::new();
        for tier in tiers.as_array()? {
            if tier["tier"]["type"] != "context" {
                return None;
            }
            let size = tier["tier"]["size"].as_u64()?;
            if !sizes.insert(size) {
                return None;
            }
            if input > size && size >= threshold {
                selected = tier;
                threshold = size;
            }
        }
    } else if input > 200_000
        && let Some(large) = cost.get("context_over_200k")
    {
        // This field is still emitted by the current upstream API, not a Sailry legacy format.
        selected = large;
    }
    selected.as_object().map(|_| selected)
}

#[cfg(test)]
mod tests;
