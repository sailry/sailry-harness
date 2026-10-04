use super::*;

pub(super) fn page(
    db: &Connection,
    query: &catalog::Query,
    revision: u64,
) -> Result<catalog::Page, Fault> {
    let mut ids = query.ids.clone();
    ids.sort();
    ids.dedup();
    let mut models = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    let mut statement = db.prepare(
        "SELECT body FROM model_catalog WHERE id=?1 AND (?2 IS NULL OR provider=?2) ORDER BY provider",
    ).map_err(storage_error)?;
    for id in ids
        .into_iter()
        .filter(|id| query.after.as_ref().is_none_or(|after| id > after))
    {
        let rows = statement
            .query_map(params![id, query.provider], |row| row.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?;
        let mut combined: Option<catalog::Model> = None;
        let mut seen = [false; 3];
        for row in rows {
            let model = metadata::model(
                &serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?,
            )?;
            if let Some(current) = &mut combined {
                merge(current, model, &mut seen);
            } else {
                seen = [
                    !model.inputs.is_empty(),
                    !model.outputs.is_empty(),
                    !model.options.is_empty(),
                ];
                combined = Some(model);
            }
        }
        let Some(model) = combined else { continue };
        let size = encode(&model)?.len() + 1;
        if models.len() == usize::from(query.limit) || bytes + size > 512 * 1024 {
            more = true;
            break;
        }
        models.push(model);
        bytes += size;
    }
    let next = more.then(|| models.last().unwrap().id.clone());
    Ok(catalog::Page {
        revision,
        models,
        next,
    })
}

// Shared identifiers can be offered with different limits by different hosts.
// Use the tighter reference, never infer capabilities from the identifier.
fn merge(current: &mut catalog::Model, other: catalog::Model, seen: &mut [bool; 3]) {
    current.context = minimum(current.context, other.context);
    current.output = minimum(current.output, other.output);
    current.tools = minimum(current.tools, other.tools);
    current.reasoning = minimum(current.reasoning, other.reasoning);
    intersect(&mut current.inputs, other.inputs, &mut seen[0]);
    intersect(&mut current.outputs, other.outputs, &mut seen[1]);
    reasoning(&mut current.options, other.options, &mut seen[2]);
}

fn reasoning(left: &mut Vec<catalog::Reasoning>, right: Vec<catalog::Reasoning>, seen: &mut bool) {
    use catalog::Reasoning;
    if right.is_empty() {
        return;
    }
    if !*seen {
        *left = right;
        *seen = true;
        return;
    }
    *left = left
        .iter()
        .filter_map(|option| {
            right.iter().find_map(|other| match (option, other) {
                (Reasoning::Effort { values }, Reasoning::Effort { values: allowed }) => {
                    let values: Vec<_> = values
                        .iter()
                        .filter(|value| allowed.contains(value))
                        .cloned()
                        .collect();
                    (!values.is_empty()).then_some(Reasoning::Effort { values })
                }
                (Reasoning::Toggle, Reasoning::Toggle) => Some(Reasoning::Toggle),
                (
                    Reasoning::BudgetTokens { min, max },
                    Reasoning::BudgetTokens {
                        min: floor,
                        max: ceiling,
                    },
                ) => {
                    let min = match (min, floor) {
                        (Some(a), Some(b)) => Some((*a).max(*b)),
                        (a, b) => a.or(*b),
                    };
                    let max = minimum(*max, *ceiling);
                    (!min
                        .zip(max)
                        .is_some_and(|(min, max)| min >= 0 && min as u32 > max))
                    .then_some(Reasoning::BudgetTokens { min, max })
                }
                _ => None,
            })
        })
        .collect();
}

fn minimum<T: Ord>(left: Option<T>, right: Option<T>) -> Option<T> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

fn intersect<T: PartialEq>(left: &mut Vec<T>, right: Vec<T>, seen: &mut bool) {
    if right.is_empty() {
        return;
    }
    if !*seen {
        *left = right;
        *seen = true;
    } else {
        left.retain(|value| right.contains(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersects_effort_values() {
        let option = |values: &[&str]| catalog::Reasoning::Effort {
            values: values.iter().map(|value| Some((*value).into())).collect(),
        };
        let mut choices = vec![option(&["none", "low", "medium", "high", "xhigh", "max"])];
        reasoning(
            &mut choices,
            vec![option(&["high", "medium", "low"])],
            &mut true,
        );
        assert_eq!(choices, vec![option(&["low", "medium", "high"])]);
    }
}
