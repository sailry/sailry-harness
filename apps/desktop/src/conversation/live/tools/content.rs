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
        for (position, block) in content.blocks.iter().enumerate() {
            let id = format!("tool-content-{key}-{position}");
            blocks.push(match block {
                Block::Text(data) => text::render(&id, data, result, literal, cx),
                Block::Notice { message } => {
                    surface::notice(message.label(&rust_i18n::locale()).to_owned(), false, cx)
                        .into_any_element()
                }
                // Published files remain visible below the answer when tool work collapses.
                Block::File(_) => continue,
                Block::Table(data) => v_flex()
                    .min_w_0()
                    .child(table::Table {
                        id: id.into(),
                        data: data.clone(),
                    })
                    .when(data.truncated, |column| {
                        column.child(surface::notice(tr("tool_partial"), false, cx))
                    })
                    .into_any_element(),
                Block::Diff(data) => {
                    let lines = diff::unified(&data.text);
                    let selector = id.clone();
                    v_flex()
                        .min_w_0()
                        .debug_selector(move || selector.clone())
                        .child(diff::file_rows(&id, &lines, &data.path, Vec::new(), cx))
                        .when(data.truncated, |column| {
                            column.child(surface::notice(tr("tool_partial"), false, cx))
                        })
                        .into_any_element()
                }
                Block::Image { index } => attachments::image_links(
                    &images
                        .iter()
                        .filter(|image| image.index == *index)
                        .cloned()
                        .collect::<Vec<_>>(),
                    page.session,
                    &self.binding,
                    &self.images,
                    cx,
                ),
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
        let copy = if literal {
            text::copy(content, result, &rust_i18n::locale())
        } else {
            serde_json::to_string_pretty(result).unwrap_or_default()
        };
        surface::result(
            key,
            v_flex()
                .min_w_0()
                .gap_2()
                .children(blocks)
                .into_any_element(),
            (!copy.is_empty()).then(|| self.copy_result(key, copy)),
            cx,
        )
        .into_any_element()
    }
}
