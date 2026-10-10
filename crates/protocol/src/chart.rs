//! Bounded chart data rendered by the framework's native chart components.
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candle {
    pub label: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub value: f64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Data {
    Line { data: Vec<Point> },
    Bar { data: Vec<Point> },
    Area { data: Vec<Point> },
    Pie { data: Vec<Point> },
    Radar { data: Vec<Point> },
    Candlestick { data: Vec<Candle> },
    Sankey { nodes: Vec<Node>, links: Vec<Link> },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    #[serde(default)]
    pub title: String,
    #[serde(flatten)]
    pub data: Data,
}

impl Chart {
    pub fn parse(source: &str) -> Option<Self> {
        if source.len() > 64 * 1024 {
            return None;
        }
        // A flattened enum cannot enforce unknown fields itself, so consume
        // the title separately and deserialize the remaining closed shape.
        let mut object =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(source).ok()?;
        let title = object
            .remove("title")
            .map_or(Some(String::new()), |value| {
                value.as_str().map(str::to_owned)
            })?;
        let data = serde_json::from_value::<Data>(serde_json::Value::Object(object)).ok()?;
        let chart = Self { title, data };
        chart.valid().then_some(chart)
    }

    pub fn valid(&self) -> bool {
        if !label(&self.title) {
            return false;
        }
        match &self.data {
            Data::Line { data } | Data::Bar { data } | Data::Area { data } => points(data),
            Data::Pie { data } | Data::Radar { data } => {
                points(data)
                    && data.iter().all(|point| point.value >= 0.0)
                    && data.iter().any(|point| point.value > 0.0)
                    && (!matches!(self.data, Data::Radar { .. }) || data.len() >= 3)
            }
            Data::Candlestick { data } => {
                !data.is_empty()
                    && data.len() <= 500
                    && data.iter().all(|candle| {
                        label(&candle.label)
                            && [candle.open, candle.high, candle.low, candle.close]
                                .into_iter()
                                .all(number)
                            && candle.low <= candle.open.min(candle.close)
                            && candle.high >= candle.open.max(candle.close)
                    })
            }
            Data::Sankey { nodes, links } => graph(nodes, links),
        }
    }

    pub fn description(&self) -> String {
        match &self.data {
            Data::Line { data }
            | Data::Bar { data }
            | Data::Area { data }
            | Data::Pie { data }
            | Data::Radar { data } => data
                .iter()
                .map(|point| format!("{}: {}", point.label, point.value))
                .collect::<Vec<_>>()
                .join("; "),
            Data::Candlestick { data } => data
                .iter()
                .map(|candle| {
                    format!(
                        "{}: {} / {} / {} / {}",
                        candle.label, candle.open, candle.high, candle.low, candle.close
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
            Data::Sankey { links, .. } => links
                .iter()
                .map(|link| format!("{} → {}: {}", link.source, link.target, link.value))
                .collect::<Vec<_>>()
                .join("; "),
        }
    }
}

fn label(value: &str) -> bool {
    value.len() <= 256 && !value.chars().any(char::is_control)
}
fn number(value: f64) -> bool {
    value.is_finite() && value.abs() <= 1e12
}
fn points(values: &[Point]) -> bool {
    !values.is_empty()
        && values.len() <= 500
        && values
            .iter()
            .all(|value| label(&value.label) && number(value.value))
}

fn graph(nodes: &[Node], links: &[Link]) -> bool {
    if nodes.is_empty() || nodes.len() > 64 || links.is_empty() || links.len() > 128 {
        return false;
    }
    let mut ids = std::collections::HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.id.is_empty()
            || !label(&node.id)
            || !label(&node.label)
            || ids.insert(&node.id, index).is_some()
        {
            return false;
        }
    }
    let mut edges = vec![Vec::new(); nodes.len()];
    let mut degrees = vec![0; nodes.len()];
    for link in links {
        let (Some(&source), Some(&target)) = (ids.get(&link.source), ids.get(&link.target)) else {
            return false;
        };
        if source == target || !number(link.value) || link.value <= 0.0 {
            return false;
        }
        edges[source].push(target);
        degrees[target] += 1;
    }
    let mut ready: Vec<_> = degrees
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect();
    let mut visited = 0;
    while let Some(node) = ready.pop() {
        visited += 1;
        for &target in &edges[node] {
            degrees[target] -= 1;
            if degrees[target] == 0 {
                ready.push(target);
            }
        }
    }
    visited == nodes.len()
}
