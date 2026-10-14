//! Kit Tree has no hierarchy strokes. Eventless borders retain its row interactions.
//! Arms follow the accepted sidebar tree: start on the parent column and stop near child content.
use super::*;

pub(super) const ROW_INSET: f32 = 8.;
const EDGE_INSET: f32 = 2.;

pub(super) fn paths(items: &[Item]) -> BTreeMap<String, Vec<bool>> {
    fn collect(items: &[Item], path: &mut Vec<bool>, paths: &mut BTreeMap<String, Vec<bool>>) {
        for (index, item) in items.iter().enumerate() {
            path.push(index + 1 < items.len());
            paths.insert(item.id.clone(), path.clone());
            collect(&item.children, path, paths);
            path.pop();
        }
    }
    let mut paths = BTreeMap::new();
    collect(items, &mut Vec::new(), &mut paths);
    paths
}

#[derive(Debug, PartialEq)]
struct Column {
    index: usize,
    half: bool,
    arm: bool,
}

fn columns(path: &[bool], skip: usize) -> Vec<Column> {
    let depth = path.len().saturating_sub(1).saturating_sub(skip);
    (0..depth)
        .filter_map(|index| {
            let current = index + 1 == depth;
            let continued = path[skip + index + 1];
            (current || continued).then_some(Column {
                index,
                half: current && !continued,
                arm: current,
            })
        })
        .collect()
}

pub(super) fn render(id: &str, path: &[bool], skip: usize, indent: f32, cx: &App) -> Vec<Div> {
    let color = cx.theme().muted_foreground.opacity(0.65);
    let mut lines = Vec::new();
    for column in columns(path, skip) {
        let left = px(ROW_INSET + indent * column.index as f32 + EDGE_INSET);
        let selector = format!("resource-guide-{id}-{}-vertical", column.index);
        lines.push(
            div()
                .debug_selector(move || selector.clone())
                .absolute()
                .left(left)
                .top_0()
                .h(relative(if column.half { 0.5 } else { 1. }))
                .border_l_1()
                .border_dashed()
                .border_color(color),
        );
        if column.arm {
            let selector = format!("resource-guide-{id}-{}-arm", column.index);
            lines.push(
                div()
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .left(left)
                    .top(relative(0.5))
                    .w(px(indent - EDGE_INSET * 2.))
                    .border_t_1()
                    .border_dashed()
                    .border_color(color),
            );
        }
    }
    lines
}

#[cfg(test)]
mod tests;
