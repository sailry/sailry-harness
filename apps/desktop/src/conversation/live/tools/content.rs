//! Every tool origin uses the same finite, native content renderer.
use super::*;
use sailry_protocol::tool::{Block, Content};
mod table;
mod text;

impl View {
    pub(super) fn structured(
        &self,
        key: &str,
        call: &Call,
        page: &Page,
        content: &Content,
        result: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let images = call.images(page);
        let literal = content
            .blocks
            .iter()
            .all(|block| matches!(block, Block::Text(_) | Block::Notice { .. }));
        let mut blocks = Vec::new();
        let mut source = content.blocks.iter().enumerate().peekable();
        while let Some((position, block)) = source.next() {
            let id = format!("tool-content-{key}-{position}");
            blocks.push(match block {
                Block::Text(data) => text::render(self, &id, call, data, result, literal, cx),
                Block::Notice { message } => {
                    surface::notice(message.label(&rust_i18n::locale()).to_owned(), cx)
                        .into_any_element()
                }
                // Published files remain visible below the answer when tool work collapses.
                Block::File(_) | Block::Artifact { .. } | Block::Card(_) => continue,
                Block::Table(data) => v_flex()
                    .min_w_0()
                    .child(table::Table {
                        id: id.into(),
                        data: data.clone(),
                    })
                    .when(data.truncated, |column| {
                        column.child(surface::notice(tr("tool_partial"), cx))
                    })
                    .into_any_element(),
                Block::Diff(data) => {
                    let selector = id.clone();
                    v_flex()
                        .min_w_0()
                        .debug_selector(move || selector.clone())
                        .child(diff::file_rows(
                            &id,
                            &data.text,
                            &data.path,
                            false,
                            Vec::new(),
                        ))
                        .when(data.truncated, |column| {
                            column.child(surface::notice(tr("tool_partial"), cx))
                        })
                        .into_any_element()
                }
                Block::Image { index } => {
                    let mut indices = vec![*index];
                    while let Some((_, Block::Image { index })) = source.peek() {
                        indices.push(*index);
                        source.next();
                    }
                    let linked: Vec<_> = indices
                        .into_iter()
                        .flat_map(|index| images.iter().filter(move |image| image.index == index))
                        .cloned()
                        .collect();
                    attachments::image_links(&linked, page.session, &self.binding, &self.images, cx)
                }
            });
        }
        if literal {
            blocks = vec![surface::scroll(
                format!("tool-content-{key}-scroll"),
                v_flex().w_full().min_w_0().children(blocks),
            )];
        }
        // Unreferenced images still belong to the result and remain inspectable.
        let remaining: Vec<_> = images
            .iter()
            .filter(|image| {
                !content
                    .blocks
                    .iter()
                    .any(|block| matches!(block, Block::Image { index } if *index == image.index))
            })
            .cloned()
            .collect();
        if !remaining.is_empty() {
            blocks.push(attachments::image_links(
                &remaining,
                page.session,
                &self.binding,
                &self.images,
                cx,
            ));
        }
        let copy = copy(content, result, &rust_i18n::locale());
        surface::result(
            key,
            v_flex()
                .min_w_0()
                .gap_2()
                .children(blocks)
                .into_any_element(),
            (!copy.is_empty()).then(|| self.copy_result(key, call, page, cx)),
            cx,
        )
        .into_any_element()
    }
}

pub(super) fn copy(content: &Content, result: &serde_json::Value, locale: &str) -> String {
    if !content
        .blocks
        .iter()
        .all(|block| matches!(block, Block::Text(_) | Block::Notice { .. }))
    {
        return serde_json::to_string_pretty(result).unwrap_or_default();
    }
    let text = content
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text(text) => text.read(result).filter(|text| !text.is_empty()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        return text;
    }
    content
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Text(text) => text
                .notices
                .iter()
                .map(|notice| notice.label(locale))
                .collect(),
            Block::Notice { message } => vec![message.label(locale)],
            _ => Vec::new(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
