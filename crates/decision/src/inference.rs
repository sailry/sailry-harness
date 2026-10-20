//! The laya/openjev subset of b11756 server-decision.cpp, with typed text inputs.
use crate::{Algorithm, Error, Metadata, Result};
use minijinja::{AutoEscape, Environment, UndefinedBehavior, value::Serde};
use sailry_protocol::decision::{Answer, Question};
use serde::Serialize;
use std::{collections::BTreeMap, io::Write};

pub(crate) struct Templates(Environment<'static>);

#[derive(Serialize)]
pub(crate) struct OptionText {
    pub key: String,
    pub description: Option<String>,
}

pub(crate) struct Query<'a> {
    pub name: &'a str,
    pub question: &'a Question,
    pub options: Vec<OptionText>,
}

impl Templates {
    pub fn new(source: &str) -> Result<Self> {
        let mut environment = Environment::new();
        environment.set_auto_escape_callback(|_| AutoEscape::None);
        environment.set_undefined_behavior(UndefinedBehavior::Strict);
        environment.set_fuel(Some(100_000));
        environment.set_recursion_limit(32);
        environment
            .add_template_owned("systemone", source.to_owned())
            .map_err(|_| Error::Invalid("unsupported systemone template"))?;
        Ok(Self(environment))
    }

    pub fn render(&self, state: &str, query: &Query<'_>, marker: &str) -> Result<String> {
        let replace = |value: &str| {
            if marker.is_empty() {
                value.to_owned()
            } else {
                value.replace(marker, " ")
            }
        };
        let options: Vec<_> = query
            .options
            .iter()
            .map(|option| OptionText {
                key: replace(&option.key),
                description: option.description.as_deref().map(replace),
            })
            .collect();
        let context = serde_json::json!({
            "id":replace(query.name), "type":query.kind(), "instructions":replace(query.instructions()),
            "state":replace(state), "options":options, "images":[]
        });
        let template = self
            .0
            .get_template("systemone")
            .map_err(|_| Error::Invalid("systemone template is missing"))?;
        let mut output = Output(Vec::new());
        template
            .render_captured_to(Serde(&context), &mut output)
            .map_err(|_| Error::Invalid("systemone template could not be rendered"))?;
        String::from_utf8(output.0).map_err(|_| Error::Invalid("systemone prompt is not UTF-8"))
    }
}

struct Output(Vec<u8>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > (256 * 1024usize).saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("systemone prompt exceeds its limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> Query<'a> {
    pub fn new(name: &'a str, question: &'a Question, algorithm: Algorithm) -> Self {
        let mut options: Vec<OptionText> = match question {
            Question::Noul { criteria, .. } => ["false", "true"]
                .into_iter()
                .map(|key| OptionText {
                    key: key.into(),
                    description: criteria.get(key).cloned(),
                })
                .collect(),
            Question::Choice { criteria, .. } => criteria
                .iter()
                .map(|(key, description)| OptionText {
                    key: key.clone(),
                    description: description.clone(),
                })
                .collect(),
            Question::Score { criteria, .. } => criteria
                .iter()
                .enumerate()
                .map(|(index, description)| OptionText {
                    key: index.to_string(),
                    description: Some(description.clone()),
                })
                .collect(),
        };
        if algorithm == Algorithm::OpenJev && matches!(question, Question::Noul { .. }) {
            options.swap(0, 1);
        }
        Self {
            name,
            question,
            options,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self.question {
            Question::Noul { .. } => "noul",
            Question::Choice { .. } => "choice",
            Question::Score { .. } => "score",
        }
    }
    fn instructions(&self) -> &str {
        match self.question {
            Question::Noul { instructions, .. }
            | Question::Choice { instructions, .. }
            | Question::Score { instructions, .. } => instructions,
        }
    }
    pub fn column(&self) -> usize {
        match self.question {
            Question::Choice { .. } => 0,
            Question::Score { .. } => 1,
            Question::Noul { .. } => 2,
        }
    }
    pub fn answer(&self, metadata: &Metadata, scores: &[f32]) -> Result<Answer> {
        let count = self.options.len();
        if scores.len() != count || scores.iter().any(|value| !value.is_finite()) {
            return Err(Error::Native("decision scores are invalid"));
        }
        let bucket = match count {
            0..=2 => "2",
            3..=5 => "3_5",
            6..=10 => "6_10",
            _ => "11",
        };
        let temperature = metadata
            .temperatures
            .get(&format!("{}.{bucket}", self.kind()))
            .or_else(|| metadata.temperatures.get(self.kind()))
            .copied()
            .unwrap_or(1.0);
        let bias = matches!(self.question, Question::Score { .. })
            .then(|| metadata.bias.get(&count))
            .flatten();
        let logits: Vec<_> = scores
            .iter()
            .enumerate()
            .map(|(index, value)| *value as f64 + bias.map_or(0.0, |values| values[index] as f64))
            .collect();
        let maximum = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut probabilities: Vec<_> = logits
            .iter()
            .map(|value| ((value - maximum) / temperature as f64).exp())
            .collect();
        let sum: f64 = probabilities.iter().sum();
        for probability in &mut probabilities {
            *probability /= sum;
        }
        let best = probabilities
            .iter()
            .enumerate()
            .max_by(|(left_index, left), (right_index, right)| {
                left.total_cmp(right)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(index, _)| index)
            .ok_or(Error::Native("decision has no options"))?;
        let mapped: BTreeMap<_, _> = self
            .options
            .iter()
            .zip(&probabilities)
            .map(|(option, probability)| (option.key.clone(), *probability as f32))
            .collect();
        Ok(match self.question {
            Question::Noul { .. } => Answer::Noul {
                noul: *mapped
                    .get("true")
                    .ok_or(Error::Native("binary option is missing"))?,
            },
            Question::Choice { .. } => {
                let uniform = 1.0 / count as f64;
                let confidence = if count < 2 {
                    1.0
                } else {
                    ((probabilities[best] - uniform) / (1.0 - uniform)).max(0.0)
                };
                Answer::Choice {
                    choice: self.options[best].key.clone(),
                    probabilities: mapped,
                    confidence: confidence as f32,
                }
            }
            Question::Score { criteria, .. } => {
                let score = probabilities
                    .iter()
                    .enumerate()
                    .map(|(index, probability)| index as f64 * probability)
                    .sum::<f64>();
                let distance: f64 = probabilities
                    .iter()
                    .enumerate()
                    .map(|(index, probability)| probability * (index as f64 - best as f64).abs())
                    .sum();
                let uniform: f64 = (0..count)
                    .map(|index| (index as f64 - (count - 1) as f64 / 2.0).abs() / count as f64)
                    .sum();
                Answer::Score {
                    score: score as f32,
                    legend: criteria.clone(),
                    probabilities: probabilities
                        .into_iter()
                        .map(|value| value as f32)
                        .collect(),
                    confidence: (1.0 - distance / uniform).max(0.0) as f32,
                }
            }
        })
    }
}

// Preserve the trained head budgets exactly; never truncate the state to fit context.
pub(crate) fn laya(
    tokens: &mut Vec<i32>,
    count: usize,
    marker: i32,
    separator: i32,
    head: usize,
) -> Result<Vec<usize>> {
    let markers: Vec<_> = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| (*token == marker).then_some(index))
        .collect();
    if count == 0
        || markers.len() != count
        || markers[0] < 2
        || tokens[markers[0] - 1] != separator
        || tokens.last() != Some(&separator)
    {
        return Err(Error::Invalid("unexpected laya prompt layout"));
    }
    let head_end = markers[0] - 1;
    let end = (markers[count - 1]..tokens.len())
        .find(|index| tokens[*index] == separator)
        .ok_or(Error::Invalid("laya option separator is missing"))?;
    if end + 1 >= tokens.len() {
        return Err(Error::Invalid("laya state is missing"));
    }
    let mut options: Vec<Vec<_>> = markers
        .iter()
        .enumerate()
        .map(|(index, start)| {
            tokens[*start..markers.get(index + 1).copied().unwrap_or(end)]
                .iter()
                .copied()
                .take(49)
                .collect()
        })
        .collect();
    let mut option_tokens: usize = options.iter().map(Vec::len).sum();
    if option_tokens + 16 > head {
        let limit = ((head - head.min(16)) / count).max(4);
        for option in &mut options {
            option.truncate(limit);
        }
        option_tokens = options.iter().map(Vec::len).sum();
    }
    let question = (head - head.min(option_tokens)).max(8);
    let mut output = Vec::with_capacity(tokens.len());
    output.push(tokens[0]);
    output.extend_from_slice(&tokens[1..head_end.min(1 + question)]);
    output.push(separator);
    let mut positions = Vec::with_capacity(count);
    for option in options {
        positions.push(output.len());
        output.extend(option);
    }
    output.extend_from_slice(&tokens[end..]);
    *tokens = output;
    Ok(positions)
}

#[cfg(test)]
mod tests;
