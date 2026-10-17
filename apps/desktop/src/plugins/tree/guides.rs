//! Kit Tree has no hierarchy strokes. An eventless canvas retains its row interactions.
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

// Dashed borders redistribute gaps for each segment's length. A fixed path
// pattern keeps seven periods per uniform row and ends its half-row on a dash.
fn pattern(height: Pixels) -> Option<[Pixels; 2]> {
    (height > px(0.)).then(|| [height / 14.; 2])
}

pub(super) fn render(id: &str, path: &[bool], skip: usize, indent: f32, cx: &App) -> Div {
    let columns = columns(path, skip);
    let color = cx.theme().border;
    let selector = format!("resource-guides-{id}");
    div()
        .debug_selector(move || selector.clone())
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            canvas(
                move |bounds, _, _| {
                    let height = bounds.size.height;
                    let Some(dashes) = pattern(height) else {
                        return Vec::new();
                    };
                    let mut paths = Vec::new();
                    for column in &columns {
                        let left = bounds.left()
                            + px(ROW_INSET + indent * column.index as f32 + EDGE_INSET + 0.5);
                        let middle = bounds.top() + height / 2.;
                        let end = if column.half { middle } else { bounds.bottom() };
                        let mut vertical = PathBuilder::stroke(px(1.)).dash_array(&dashes);
                        vertical.move_to(point(left, bounds.top()));
                        vertical.line_to(point(left, end));
                        if let Ok(path) = vertical.build() {
                            paths.push(path);
                        }
                        if column.arm {
                            // Each arm starts on the same dash as its vertical join;
                            // short arms clip the pattern instead of stretching gaps.
                            let mut arm = PathBuilder::stroke(px(1.)).dash_array(&dashes);
                            arm.move_to(point(left, middle));
                            arm.line_to(point(left + px(indent - EDGE_INSET * 2.), middle));
                            if let Ok(path) = arm.build() {
                                paths.push(path);
                            }
                        }
                    }
                    paths
                },
                move |bounds, paths, window, _| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        for path in paths {
                            window.paint_path(path, color);
                        }
                    });
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests;
