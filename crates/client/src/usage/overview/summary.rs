use super::*;
use sailry_protocol::conversation::Usage;

pub(super) fn combine(sources: &[Source]) -> Result<Option<Summary>, Fault> {
    let mut totals = Metrics::default();
    let mut days = BTreeMap::<i64, Point>::new();
    let mut recent = BTreeMap::<i64, Point>::new();
    let mut groups = Vec::new();
    let mut requests = BTreeMap::new();
    let mut has_more = false;
    let mut available = false;
    for source in sources {
        let Some(report) = &source.view.report else {
            continue;
        };
        available = true;
        has_more |= report.requests.has_more;
        for request in &report.requests.items {
            requests.insert(request.position, request.clone());
            if requests.len() > sailry_protocol::usage::PAGE_SIZE {
                requests.pop_first();
                has_more = true;
            }
        }
        add(&mut totals, &report.totals)?;
        for (target, points) in [(&mut days, &report.days), (&mut recent, &report.recent)] {
            for point in points {
                let bucket = target.entry(point.start_ms).or_insert_with(|| Point {
                    start_ms: point.start_ms,
                    metrics: Metrics::default(),
                    models: BTreeMap::new(),
                });
                add(&mut bucket.metrics, &point.metrics)?;
                for (model, tokens) in &point.models {
                    match bucket.models.entry(model.clone()) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(tokens.clone());
                        }
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            add_tokens(entry.get_mut(), tokens)?
                        }
                    }
                }
            }
        }
        groups.extend(report.groups.iter().map(|group| ScopedGroup {
            node: source.node,
            group: group.clone(),
        }));
    }
    Ok(available.then(|| Summary {
        totals,
        days: days.into_values().collect(),
        recent: recent.into_values().collect(),
        groups,
        requests: sailry_protocol::usage::Page {
            items: requests.into_values().rev().collect(),
            has_more,
        },
    }))
}

fn add(total: &mut Metrics, source: &Metrics) -> Result<(), Fault> {
    let add = |target: &mut u64, value: u64| -> Result<(), Fault> {
        *target = target.checked_add(value).ok_or_else(|| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "usage overview exceeds the counter limit",
            )
        })?;
        Ok(())
    };
    add(&mut total.responses, source.responses)?;
    if let Some(generation) = &source.generation {
        let total = total.generation.get_or_insert_default();
        add(&mut total.elapsed_us, generation.elapsed_us)?;
        add(&mut total.output_tokens, generation.output_tokens)?;
        add(&mut total.responses, generation.responses)?;
    }
    if let Some(cost) = &source.cost {
        total.cost = Some(match &total.cost {
            Some(total) => total.checked_add(cost).ok_or_else(|| {
                Fault::new(
                    ErrorCode::InvalidRequest,
                    "usage cost exceeds the counter limit",
                )
            })?,
            None => cost.clone(),
        });
    }
    if let Some(tokens) = &source.tokens {
        let total = total.tokens.get_or_insert(Usage {
            input: 0,
            output: 0,
            cached_input: 0,
            reasoning: 0,
        });
        add_tokens(total, tokens)?;
    }
    Ok(())
}

fn add_tokens(total: &mut Usage, source: &Usage) -> Result<(), Fault> {
    for (target, value) in [
        (&mut total.input, source.input),
        (&mut total.output, source.output),
        (&mut total.cached_input, source.cached_input),
        (&mut total.reasoning, source.reasoning),
    ] {
        *target = target.checked_add(value).ok_or_else(|| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "usage overview exceeds the counter limit",
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::usage::{Cost, CostBreakdown};

    #[test]
    fn preserves_partial_costs() {
        let source = Metrics {
            responses: 1,
            cost: Some(Cost {
                usd_micros: 10,
                responses: 1,
                breakdown: Some(CostBreakdown {
                    input: 4,
                    output: 3,
                    cache_read: 2,
                    cache_write: 1,
                }),
            }),
            ..Default::default()
        };
        let mut total = Metrics::default();
        add(&mut total, &source).unwrap();
        add(&mut total, &source).unwrap();
        let cost = total.cost.as_ref().unwrap();
        assert_eq!(cost.usd_micros, 20);
        assert_eq!(cost.breakdown.as_ref().unwrap().cache_read, 4);
        let reported = Metrics {
            responses: 1,
            cost: Some(Cost {
                usd_micros: 5,
                responses: 1,
                breakdown: None,
            }),
            ..Default::default()
        };
        add(&mut total, &reported).unwrap();
        assert_eq!(total.cost.as_ref().unwrap().usd_micros, 25);
        assert_eq!(total.cost.as_ref().unwrap().breakdown, None);
    }
}
