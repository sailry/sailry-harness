//! Captured, portable data recipes reuse the ordinary conversation surfaces.
use super::*;
use sailry_protocol::tool::projection::{Format, Output as Projection};
use serde_json::Value;

pub(super) struct Group {
    pub count: usize,
    pub content: AnyElement,
}

fn descriptor<'a>(call: &Call, page: &'a Page) -> Option<&'a Projection> {
    call.display(page)?.output.as_ref()
}

pub(super) fn grouped(call: &Call, page: &Page) -> bool {
    descriptor(call, page).is_some_and(|projection| projection.paths.is_some())
}

fn paths(calls: &[&Call], page: &Page) -> Vec<SharedString> {
    let mut paths = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for call in calls {
        let Some(source) = descriptor(call, page).and_then(|projection| projection.paths.as_ref())
        else {
            continue;
        };
        for path in source.read(
            call.arguments(page).unwrap_or(&Value::Null),
            call.result(page).unwrap_or(&Value::Null),
        ) {
            if seen.insert(path.to_owned()) {
                paths.push(SharedString::from(path.to_owned()));
            }
        }
    }
    paths
}

pub(super) fn group(
    view: &View,
    calls: &[&Call],
    page: &Page,
    cx: &mut Context<View>,
) -> Option<Group> {
    let first = *calls.first()?;
    if !grouped(first, page) {
        return None;
    }
    let paths = paths(calls, page);
    let failures: Vec<_> = calls
        .iter()
        .filter_map(|call| {
            let result = call.result(page)?;
            (output::is_error(&call.name, Some(result))
                || output::failure(&call.name, Some(result)).is_some())
            .then_some((*call, result))
        })
        .collect();
    let worktree = view.turn_worktree(first.turn);
    Some(Group {
        count: paths.len().max(failures.len()),
        content: v_flex()
            .gap_2()
            .children(paths.into_iter().enumerate().map(|(index, path)| {
                disclosure::trigger(
                    format!(
                        "live-tool-file-{}-{}-{index}",
                        first.turn,
                        first.source.key()
                    ),
                    IconName::FileText,
                    path.clone(),
                    Detail {
                        secondary: true,
                        ..Default::default()
                    },
                    false,
                    cx,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    if let Some(worktree) = worktree {
                        cx.emit(Event::FileAt(worktree, path.to_string()));
                    }
                }))
            }))
            .children(failures.into_iter().map(|(call, result)| {
                view.generic(
                    &format!("{}-{}", call.turn, call.source.key()),
                    call,
                    None,
                    result,
                    cx,
                )
            }))
            .into_any_element(),
    })
}

pub(super) fn render(
    view: &View,
    key: &str,
    call: &Call,
    page: &Page,
    result: &Value,
    cx: &mut Context<View>,
) -> Option<AnyElement> {
    if output::is_error(&call.name, Some(result))
        || output::failure(&call.name, Some(result)).is_some()
    {
        return None;
    }
    let projection = descriptor(call, page)?;
    let arguments = call.arguments(page).unwrap_or(&Value::Null);
    if let Some(table) = &projection.table {
        let data = table.render(arguments, result)?;
        let content = sailry_protocol::tool::Content {
            version: 1,
            blocks: vec![sailry_protocol::tool::Block::Table(data)],
        };
        return Some(view.structured(key, call, page, &content, result, cx));
    }
    if let Some(body) = &projection.body {
        let text = body.text.read(arguments, result)?.as_str()?;
        let path = body
            .path
            .as_ref()
            .and_then(|path| path.read(arguments, result)?.as_str())
            .filter(|path| !path.is_empty());
        let mut actions = Vec::new();
        if !text.is_empty() {
            actions.push(view.copy_result(key, text.to_owned()));
        }
        if let Some(path) = path {
            actions.push(view.path_link(key, call.turn, path.to_owned().into(), cx));
        }
        if let Some(style) = &body.diff
            && style
                .when
                .as_ref()
                .is_none_or(|test| test.matches(arguments, result))
        {
            let lines = match style.format {
                Format::Added => diff::additions(text),
                Format::Unified => diff::unified(text),
            };
            return Some(diff::file_rows(
                key,
                &lines,
                path.unwrap_or_default(),
                actions,
                cx,
            ));
        }
        let content = if text.is_empty() {
            v_flex()
                .children(body.empty.as_ref().map(|empty| {
                    surface::notice(empty.label(&rust_i18n::locale()).to_owned(), false, cx)
                }))
                .into_any_element()
        } else {
            surface::code(&format!("{key}-output"), text, cx)
        };
        return Some(surface::result(key, content, actions, cx).into_any_element());
    }
    if projection.preview.is_none() && projection.notices.is_empty() {
        return None;
    }
    let text = projection
        .preview
        .as_ref()
        .map(|text| text.render(arguments, result))
        .unwrap_or_default();
    let notices = projection.notices(arguments, result, &rust_i18n::locale());
    let copy = if text.is_empty() {
        notices.join("\n")
    } else {
        text.clone()
    };
    let content = v_flex()
        .when(!text.is_empty(), |column| {
            column.child(surface::code(&format!("{key}-output"), &text, cx))
        })
        .children(
            notices
                .into_iter()
                .map(|notice| surface::notice(notice, false, cx)),
        );
    let mut actions = Vec::new();
    if !copy.is_empty() {
        actions.push(view.copy_result(key, copy));
    }
    if let Some(path) = call
        .display(page)
        .and_then(|display| display.input.as_ref())
        .and_then(|input| input.target.as_ref())
        .and_then(|path| path.read(arguments))
    {
        actions.push(view.path_link(key, call.turn, path.to_owned().into(), cx));
    }
    Some(surface::result(key, content.into_any_element(), actions, cx).into_any_element())
}

#[cfg(test)]
mod tests;
