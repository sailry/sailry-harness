use super::unavailable;
use sailry_protocol::{Effort, Fault, conversation::catalog::EffortScore};
use serde::Deserialize;
use std::collections::BTreeMap;

const AXES: [&str; 5] = [
    "backend",
    "frontend",
    "knowledge",
    "aesthetic",
    "maintainability",
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Dataset {
    pub batch: String,
    pub published_at: String,
    pub entries: Vec<Entry>,
    pub aliases: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub model: String,
    pub effort: String,
    pub score: EffortScore,
}

#[derive(Deserialize)]
struct Document {
    snapshot: Snapshot,
    #[serde(default)]
    metrics: BTreeMap<String, BTreeMap<String, Metric>>,
    #[serde(default, rename = "displayNames")]
    names: BTreeMap<String, String>,
    #[serde(default, rename = "effortDisplayNames")]
    efforts: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct Snapshot {
    batch_id: String,
    published_at: String,
    entries: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    candidate_id: String,
    model_id: String,
    reasoning_effort: String,
    overall_score: Option<f32>,
    backend_score: Option<f64>,
    frontend_score: Option<f64>,
    knowledge_score: Option<f64>,
    aesthetic_score: Option<f64>,
    maintainability_score: Option<f64>,
}

#[derive(Deserialize)]
struct Metric {
    elapsed_ms: Option<u64>,
    estimated_api_cost_usd: Option<f32>,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Dataset, Fault> {
    let document: Document =
        serde_json::from_slice(bytes).map_err(|_| unavailable("model scores JSON is invalid"))?;
    let snapshot = document.snapshot;
    if snapshot.entries.len() > 2000
        || snapshot.batch_id.is_empty()
        || snapshot.batch_id.len() > 256
        || snapshot.published_at.len() > 64
        || snapshot.published_at.is_empty()
    {
        return Err(unavailable("model scores snapshot is invalid"));
    }
    let mut rows = snapshot.entries;
    rows.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    let mut entries = BTreeMap::new();
    for row in rows {
        let Some(overall) = row.overall_score else {
            continue;
        };
        if !overall.is_finite()
            || !(0.0..=100.0).contains(&overall)
            || row.model_id.is_empty()
            || row.model_id.len() > 256
        {
            // One malformed row must not discard the rest of the snapshot.
            continue;
        }
        let axes = [
            row.backend_score,
            row.frontend_score,
            row.knowledge_score,
            row.aesthetic_score,
            row.maintainability_score,
        ]
        .map(|score| {
            score
                .filter(|score| score.is_finite() && (0.0..=100.0).contains(score))
                .map(|score| score.round() as u8)
        });
        let [
            Some(backend),
            Some(frontend),
            Some(knowledge),
            Some(aesthetic),
            Some(maintainability),
        ] = axes
        else {
            continue;
        };
        let effort = document
            .efforts
            .get(&row.model_id)
            .and_then(|efforts| efforts.get(&row.reasoning_effort))
            .unwrap_or(&row.reasoning_effort);
        let effort = match effort.as_str() {
            "low" => Effort::Low,
            "medium" => Effort::Medium,
            "high" => Effort::High,
            "xhigh" => Effort::XHigh,
            "max" => Effort::Max,
            "default" | "unknown" => Effort::Default,
            _ => continue,
        };
        let key = serde_json::to_value(effort)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        // Metrics are optional context; an invalid value drops only that total.
        let metrics: Vec<_> = AXES
            .iter()
            .filter_map(|axis| document.metrics.get(&row.candidate_id)?.get(*axis))
            .collect();
        let elapsed_ms = metrics
            .iter()
            .filter_map(|metric| metric.elapsed_ms)
            .try_fold(None::<u64>, |total, elapsed| {
                total.unwrap_or(0).checked_add(elapsed).map(Some)
            })
            .flatten();
        let cost_usd = metrics
            .iter()
            .filter_map(|metric| metric.estimated_api_cost_usd)
            .try_fold(None::<f32>, |total, cost| {
                let total = total.unwrap_or(0.0) + cost;
                (cost.is_finite() && cost >= 0.0 && total.is_finite()).then_some(Some(total))
            })
            .flatten();
        entries
            .entry((row.model_id.clone(), key.clone()))
            .or_insert(Entry {
                model: row.model_id,
                effort: key,
                score: EffortScore {
                    effort,
                    overall,
                    axes: [backend, frontend, knowledge, aesthetic, maintainability],
                    elapsed_ms,
                    cost_usd,
                },
            });
    }
    let models: std::collections::BTreeSet<_> = entries.keys().map(|(model, _)| model).collect();
    let mut aliases = BTreeMap::new();
    let mut conflicts = std::collections::BTreeSet::new();
    for (model, alias) in document.names {
        if models.contains(&model) {
            let alias = super::matching::normalize(&alias);
            if aliases.insert(alias.clone(), model).is_some() {
                conflicts.insert(alias);
            }
        }
    }
    aliases.retain(|alias, _| !conflicts.contains(alias));
    Ok(Dataset {
        batch: snapshot.batch_id,
        published_at: snapshot.published_at,
        entries: entries.into_values().collect(),
        aliases,
    })
}
