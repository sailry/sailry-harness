//! Graph views over resolved references. Every read is bounded; reaching a
//! bound marks the page partial and truncated instead of failing it.
use super::super::{index::Reader, parser::Usage};
use super::{
    rank,
    resolve::{self, Candidate, Site},
};
use sailry_protocol::{Fault, code_index::*};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
};

/// Definitions selected for callers, callees and context.
const MAX_TARGETS: usize = 20;
/// Definitions seeding impact.
const MAX_SEEDS: usize = 512;
/// Reference rows read per lookup or hop.
const MAX_SCAN: usize = 20_000;
/// Definitions carried into the next impact hop.
const MAX_FRONTIER: usize = 512;
const MAX_SAMPLES: usize = 3;
const MAX_AFFECTED: usize = 10;
const MAX_RANKED: usize = 8;
/// Grouped file and name reference counts read for the map.
const MAX_MAP_ROWS: usize = 200_000;

/// Path, enclosing scope name and start line, then symbol name and kind.
type UsageSite = (String, Option<(String, u32)>, String, String);
const MAX_MAP_IMPORTS: usize = 200_000;

type Key = (String, String, u32);
type Strength = (Confidence, bool);

pub(in super::super) async fn run(reader: &Reader<'_>, query: &Graph) -> Result<GraphPage, Fault> {
    let mut page = GraphPage::new(query.view);
    if query.view == View::Map {
        map(reader, query, &mut page).await?;
        return Ok(page);
    }
    let mut outlines = Outlines::default();
    let limit = if query.view == View::Impact {
        MAX_SEEDS
    } else {
        MAX_TARGETS
    };
    let targets = targets(reader, query, limit).await?;
    page.targets = targets.iter().map(Candidate::definition).collect();
    match query.view {
        View::Callers => {
            let found = callers(reader, &targets, &mut outlines).await?;
            page.callers = paged(found, query, &mut page);
        }
        View::Callees => {
            let found = callees(reader, &targets, &mut outlines).await?;
            page.callees = paged(found, query, &mut page);
        }
        View::Impact => {
            let found = impact(reader, &targets, query.depth, &mut outlines).await?;
            page.impact = paged(found, query, &mut page);
        }
        View::Context | View::Map => {
            let mut callers = callers(reader, &targets, &mut outlines).await?;
            let mut callees = callees(reader, &targets, &mut outlines).await?;
            let mut impact = impact(reader, &targets, query.depth, &mut outlines).await?;
            page.total = impact.len();
            page.truncated |= [callers.len(), callees.len(), impact.len()]
                .into_iter()
                .any(|count| count > query.limit);
            callers.truncate(query.limit);
            callees.truncate(query.limit);
            impact.truncate(query.limit);
            page.callers = callers;
            page.callees = callees;
            page.impact = impact;
        }
    }
    Ok(page)
}

fn paged<T>(items: Vec<T>, query: &Graph, page: &mut GraphPage) -> Vec<T> {
    page.total = items.len();
    let end = query.offset.saturating_add(query.limit);
    if end < items.len() {
        page.next_offset = Some(end);
        page.truncated = true;
    }
    items
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .collect()
}

/// Definitions selected by name, path and line.
async fn targets(
    reader: &Reader<'_>,
    query: &Graph,
    limit: usize,
) -> Result<Vec<Candidate>, Fault> {
    let mut found = match (&query.name, query.line) {
        (Some(name), _) => {
            let names = BTreeSet::from([name.clone()]);
            let mut found = reader
                .definitions(&names)
                .await?
                .remove(name)
                .unwrap_or_default();
            if !query.paths.is_empty() {
                found.retain(|candidate| {
                    query.paths.iter().any(|path| within(&candidate.path, path))
                });
            }
            found
        }
        (None, Some(_)) => reader
            .outline(&query.paths[0])
            .await?
            .into_iter()
            .filter(|candidate| resolve::definition(&candidate.kind))
            .collect(),
        (None, None) => reader.within(&query.paths, limit + 1).await?,
    };
    if let Some(line) = query.line {
        let innermost = found
            .iter()
            .filter(|candidate| candidate.start_line <= line && line <= candidate.end_line)
            .max_by_key(|candidate| (candidate.start_line, Reverse(candidate.end_line)))
            .cloned();
        found = innermost.into_iter().collect();
    }
    if found.len() > limit {
        found.truncate(limit);
        reader.limit();
    }
    Ok(found)
}

/// References resolving to any target, grouped by place.
async fn callers(
    reader: &Reader<'_>,
    targets: &[Candidate],
    outlines: &mut Outlines,
) -> Result<Vec<Reference>, Fault> {
    let names: BTreeSet<String> = targets.iter().map(|target| target.name.clone()).collect();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let usages = reader.references(&names, MAX_SCAN).await?;
    let candidates = reader.definitions(&names).await?;
    let paths = usages.iter().map(|(path, _)| path.clone()).collect();
    let imports = reader.imports(&paths).await?;
    let wanted: BTreeSet<(&str, &str, u32)> = targets.iter().map(Candidate::key).collect();
    let mut grouped: BTreeMap<UsageSite, Reference> = BTreeMap::new();
    for (path, usage) in &usages {
        reader.check()?;
        let Some((confidence, chosen)) =
            resolved(reader, path, usage, &candidates, &imports, outlines).await?
        else {
            continue;
        };
        let hits: Vec<&Candidate> = chosen
            .iter()
            .copied()
            .filter(|candidate| wanted.contains(&candidate.key()))
            .collect();
        if hits.is_empty() {
            continue;
        }
        let key = (
            path.clone(),
            usage
                .scope
                .as_ref()
                .map(|scope| (scope.name.clone(), scope.start_line)),
            usage.name.clone(),
            usage.kind.clone(),
        );
        grouped
            .entry(key)
            .and_modify(|reference| reference.count = reference.count.saturating_add(1))
            .or_insert_with(|| reference(path, usage, Some(confidence), &chosen, &hits));
    }
    let mut found: Vec<Reference> = grouped.into_values().collect();
    found.sort_by(|left, right| order(left).cmp(&order(right)));
    Ok(found)
}

/// References inside the targets with their resolved definitions.
async fn callees(
    reader: &Reader<'_>,
    targets: &[Candidate],
    outlines: &mut Outlines,
) -> Result<Vec<Reference>, Fault> {
    let mut usages = Vec::new();
    for target in targets {
        for usage in reader
            .inside(&target.path, target.start, target.end, MAX_SCAN)
            .await?
        {
            usages.push((target.path.clone(), usage));
        }
    }
    let names: BTreeSet<String> = usages.iter().map(|(_, usage)| usage.name.clone()).collect();
    let candidates = reader.definitions(&names).await?;
    let paths = targets.iter().map(|target| target.path.clone()).collect();
    let imports = reader.imports(&paths).await?;
    let mut grouped: BTreeMap<(String, String, String), Reference> = BTreeMap::new();
    for (path, usage) in &usages {
        reader.check()?;
        let (confidence, chosen) =
            match resolved(reader, path, usage, &candidates, &imports, outlines).await? {
                Some((confidence, chosen)) => (Some(confidence), chosen),
                None => (None, Vec::new()),
            };
        grouped
            .entry((path.clone(), usage.name.clone(), usage.kind.clone()))
            .and_modify(|reference| reference.count = reference.count.saturating_add(1))
            .or_insert_with(|| reference(path, usage, confidence, &chosen, &chosen));
    }
    let mut found: Vec<Reference> = grouped.into_values().collect();
    found.sort_by(|left, right| {
        (
            left.confidence.is_none(),
            left.confidence,
            left.ambiguous,
            left.line,
        )
            .cmp(&(
                right.confidence.is_none(),
                right.confidence,
                right.ambiguous,
                right.line,
            ))
    });
    Ok(found)
}

/// Definitions reached by reverse references within `depth` hops. Each keeps
/// its shortest hop count and, at that hop, the strongest resolution path.
async fn impact(
    reader: &Reader<'_>,
    seeds: &[Candidate],
    depth: u8,
    outlines: &mut Outlines,
) -> Result<Vec<Impacted>, Fault> {
    let seeded: BTreeSet<Key> = seeds.iter().map(owned).collect();
    let mut frontier: BTreeMap<Key, Strength> = seeded
        .iter()
        .map(|key| (key.clone(), (Confidence::Local, false)))
        .collect();
    let mut reached: BTreeMap<Key, Affected> = BTreeMap::new();
    let mut files: BTreeMap<String, (u8, Strength)> = BTreeMap::new();
    for hop in 1..=depth {
        if frontier.is_empty() {
            break;
        }
        let names: BTreeSet<String> = frontier.keys().map(|(_, name, _)| name.clone()).collect();
        let usages = reader.references(&names, MAX_SCAN).await?;
        let candidates = reader.definitions(&names).await?;
        let paths = usages.iter().map(|(path, _)| path.clone()).collect();
        let imports = reader.imports(&paths).await?;
        let mut next: BTreeMap<Key, Strength> = BTreeMap::new();
        for (path, usage) in &usages {
            reader.check()?;
            let Some((confidence, chosen)) =
                resolved(reader, path, usage, &candidates, &imports, outlines).await?
            else {
                continue;
            };
            let Some(via) = chosen
                .iter()
                .filter_map(|candidate| frontier.get(&owned(candidate)))
                .copied()
                .min()
            else {
                continue;
            };
            let strength = (via.0.max(confidence), via.1 || chosen.len() > 1);
            let Some(scope) = &usage.scope else {
                let entry = files.entry(path.clone()).or_insert((hop, strength));
                if (hop, strength) < *entry {
                    *entry = (hop, strength);
                }
                continue;
            };
            let key = (path.clone(), scope.name.clone(), scope.start_line);
            if seeded.contains(&key) {
                continue;
            }
            let known = reached.entry(key.clone()).or_insert_with(|| Affected {
                name: scope.name.clone(),
                kind: scope.kind.clone(),
                start_line: scope.start_line,
                end_line: scope.end_line,
                depth: hop,
                confidence: strength.0,
                ambiguous: strength.1,
            });
            if known.depth != hop {
                continue;
            }
            if strength < (known.confidence, known.ambiguous) {
                known.confidence = strength.0;
                known.ambiguous = strength.1;
            }
            if let Some(current) = next.get_mut(&key) {
                *current = (*current).min(strength);
            } else if next.len() < MAX_FRONTIER {
                next.insert(key, strength);
            } else {
                reader.limit();
            }
        }
        frontier = next;
    }
    let mut grouped: BTreeMap<String, Impacted> = BTreeMap::new();
    for ((path, _, _), affected) in reached {
        let strength = (affected.confidence, affected.ambiguous);
        let item = grouped
            .entry(path.clone())
            .or_insert_with(|| impacted(path, affected.depth, strength));
        merge(item, affected.depth, strength);
        item.symbols.push(affected);
    }
    for (path, (hop, strength)) in files {
        let item = grouped
            .entry(path.clone())
            .or_insert_with(|| impacted(path, hop, strength));
        merge(item, hop, strength);
    }
    let mut found: Vec<Impacted> = grouped.into_values().collect();
    for item in &mut found {
        item.symbols.sort_by(|left, right| {
            (left.depth, left.confidence, left.ambiguous, left.start_line).cmp(&(
                right.depth,
                right.confidence,
                right.ambiguous,
                right.start_line,
            ))
        });
        item.truncated = item.symbols.len() > MAX_AFFECTED;
        item.symbols.truncate(MAX_AFFECTED);
    }
    found.sort_by(|left, right| {
        (left.depth, left.confidence, left.ambiguous, &left.path).cmp(&(
            right.depth,
            right.confidence,
            right.ambiguous,
            &right.path,
        ))
    });
    Ok(found)
}

/// Files ranked by personalized PageRank over weighted references.
async fn map(reader: &Reader<'_>, query: &Graph, page: &mut GraphPage) -> Result<(), Fault> {
    let totals = reader.totals(MAX_MAP_ROWS).await?;
    let counts = reader.counts().await?;
    let names: BTreeSet<String> = totals.iter().map(|(_, name, _)| name.clone()).collect();
    let candidates = reader.definitions(&names).await?;
    let imports = reader.modules(MAX_MAP_IMPORTS).await?;
    let mut nodes: BTreeMap<String, usize> = counts.keys().map(|path| (path.clone(), 0)).collect();
    for (path, _, _) in &totals {
        nodes.entry(path.clone()).or_insert(0);
    }
    for (index, value) in nodes.values_mut().enumerate() {
        *value = index;
    }
    let mut weights: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    let mut inbound: BTreeMap<(usize, String), f64> = BTreeMap::new();
    for (path, name, count) in &totals {
        reader.check()?;
        let Some(all) = candidates.get(name) else {
            continue;
        };
        let site = Site {
            path,
            start: 0,
            receiver: false,
            imports: imports.get(path).map(Vec::as_slice).unwrap_or_default(),
        };
        let Some((_, chosen)) = resolve::resolve(&site, all) else {
            continue;
        };
        let from = nodes[path];
        // Dampened like Aider: many uses of one name should not dominate.
        let weight = f64::from(*count).sqrt() / chosen.len() as f64;
        for candidate in chosen {
            if candidate.path == *path {
                continue;
            }
            let Some(&to) = nodes.get(&candidate.path) else {
                continue;
            };
            *weights.entry((from, to)).or_default() += weight;
            *inbound.entry((to, candidate.name.clone())).or_default() += weight;
        }
    }
    let paths: Vec<&String> = nodes.keys().collect();
    let edges: Vec<(usize, usize, f64)> = weights
        .into_iter()
        .map(|((from, to), weight)| (from, to, weight))
        .collect();
    let focus: Vec<usize> = nodes
        .iter()
        .filter(|(path, _)| query.paths.iter().any(|prefix| within(path, prefix)))
        .map(|(_, index)| *index)
        .collect();
    let ranks = rank::pagerank(nodes.len(), &edges, &focus);
    let mut order: Vec<usize> = (0..paths.len())
        .filter(|index| counts.get(paths[*index]).is_some_and(|count| *count > 0))
        .collect();
    order.sort_by(|left, right| {
        ranks[*right]
            .total_cmp(&ranks[*left])
            .then_with(|| paths[*left].cmp(paths[*right]))
    });
    let top = order.first().map_or(0.0, |index| ranks[*index]);
    let end = query.offset.saturating_add(query.limit);
    page.total = order.len();
    if end < order.len() {
        page.next_offset = Some(end);
        page.truncated = true;
    }
    for index in order.into_iter().skip(query.offset).take(query.limit) {
        let path = paths[index];
        let mut named: Vec<(&String, f64)> = inbound
            .range((index, String::new())..)
            .take_while(|((to, _), _)| *to == index)
            .map(|((_, name), weight)| (name, *weight))
            .collect();
        named.sort_by(|left, right| right.1.total_cmp(&left.1).then_with(|| left.0.cmp(right.0)));
        let outline = reader.outline(path).await?;
        let mut symbols: Vec<Ranked> = named
            .iter()
            .filter_map(|(name, weight)| {
                outline
                    .iter()
                    .find(|candidate| {
                        candidate.name == **name && resolve::definition(&candidate.kind)
                    })
                    .map(|candidate| ranked(candidate, weight.round() as u32))
            })
            .take(MAX_RANKED)
            .collect();
        if symbols.is_empty() {
            symbols = outline
                .iter()
                .filter(|candidate| candidate.depth == 0 && resolve::definition(&candidate.kind))
                .take(MAX_RANKED)
                .map(|candidate| ranked(candidate, 0))
                .collect();
        }
        let score = if top > 0.0 {
            (ranks[index] / top * 1000.0).round() as u32
        } else {
            0
        };
        page.map.push(Entry {
            path: path.clone(),
            score,
            symbols,
        });
    }
    Ok(())
}

/// Resolves one reference, narrowing same-file ambiguity lexically.
async fn resolved<'c>(
    reader: &Reader<'_>,
    path: &str,
    usage: &Usage,
    candidates: &'c BTreeMap<String, Vec<Candidate>>,
    imports: &BTreeMap<String, Vec<String>>,
    outlines: &mut Outlines,
) -> Result<Option<(Confidence, Vec<&'c Candidate>)>, Fault> {
    let Some(all) = candidates.get(&usage.name) else {
        return Ok(None);
    };
    let site = Site {
        path,
        start: usage.start,
        receiver: usage.receiver,
        imports: imports.get(path).map(Vec::as_slice).unwrap_or_default(),
    };
    let Some((confidence, chosen)) = resolve::resolve(&site, all) else {
        return Ok(None);
    };
    if confidence != Confidence::Local || chosen.len() < 2 {
        return Ok(Some((confidence, chosen)));
    }
    let outline = outlines.load(reader, path).await?;
    Ok(Some((confidence, resolve::narrow(&site, chosen, outline))))
}

/// Declarations of files already read by this query.
#[derive(Default)]
struct Outlines(BTreeMap<String, Vec<Candidate>>);

impl Outlines {
    async fn load(&mut self, reader: &Reader<'_>, path: &str) -> Result<&[Candidate], Fault> {
        if !self.0.contains_key(path) {
            let outline = reader.outline(path).await?;
            self.0.insert(path.to_owned(), outline);
        }
        Ok(self.0.get(path).map(Vec::as_slice).unwrap_or_default())
    }
}

fn reference(
    path: &str,
    usage: &Usage,
    confidence: Option<Confidence>,
    chosen: &[&Candidate],
    shown: &[&Candidate],
) -> Reference {
    Reference {
        path: path.to_owned(),
        name: usage.name.clone(),
        kind: usage.kind.clone(),
        line: usage.line,
        column: usage.column,
        count: 1,
        scope: usage.scope.as_ref().map(|scope| Definition {
            path: path.to_owned(),
            name: scope.name.clone(),
            kind: scope.kind.clone(),
            start_line: scope.start_line,
            end_line: scope.end_line,
        }),
        targets: shown
            .iter()
            .take(MAX_SAMPLES)
            .map(|candidate| candidate.definition())
            .collect(),
        candidates: chosen.len(),
        confidence,
        ambiguous: chosen.len() > 1,
    }
}

/// Calls first, then stronger and unambiguous resolutions, then position.
fn order(reference: &Reference) -> (u8, Option<Confidence>, bool, &str, u32) {
    let kind = match reference.kind.as_str() {
        "call" | "send" => 0,
        "class" => 1,
        "implementation" | "interface" => 2,
        "type" => 3,
        "import" => 5,
        _ => 4,
    };
    (
        kind,
        reference.confidence,
        reference.ambiguous,
        &reference.path,
        reference.line,
    )
}

fn impacted(path: String, depth: u8, strength: Strength) -> Impacted {
    Impacted {
        path,
        depth,
        confidence: strength.0,
        ambiguous: strength.1,
        symbols: Vec::new(),
        truncated: false,
    }
}

fn merge(item: &mut Impacted, depth: u8, strength: Strength) {
    item.depth = item.depth.min(depth);
    if strength < (item.confidence, item.ambiguous) {
        item.confidence = strength.0;
        item.ambiguous = strength.1;
    }
}

fn ranked(candidate: &Candidate, references: u32) -> Ranked {
    Ranked {
        name: candidate.name.clone(),
        kind: candidate.kind.clone(),
        start_line: candidate.start_line,
        end_line: candidate.end_line,
        signature: candidate.signature.clone(),
        references,
    }
}

fn owned(candidate: &Candidate) -> Key {
    (
        candidate.path.clone(),
        candidate.name.clone(),
        candidate.start_line,
    )
}

fn within(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}
