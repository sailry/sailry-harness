use super::*;
use gpui_kit::component::input::{CompletionProvider, DefinitionProvider, HoverProvider};
use lsp_types::{
    CompletionContext, CompletionResponse, Hover, HoverContents, MarkupContent, MarkupKind,
};
use ropey::Rope;

impl HoverProvider for Provider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Option<Hover>>> {
        let task = self.request(Kind::Hover, text, offset, cx);
        cx.spawn(async move |_| {
            let result = task.await?;
            Ok(match result.content {
                code::Content::Hover(value) if !value.is_empty() => Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::PlainText,
                        value,
                    }),
                    range: None,
                }),
                _ => None,
            })
        })
    }
}

impl DefinitionProvider for Provider {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<lsp_types::LocationLink>>> {
        let task = self.request(Kind::Definition, text, offset, cx);
        cx.spawn(async move |_| {
            let result = task.await?;
            let code::Content::Locations(values) = result.content else {
                return Ok(vec![]);
            };
            Ok(values
                .into_iter()
                .filter_map(|item| {
                    Some(lsp_types::LocationLink {
                        origin_selection_range: None,
                        target_uri: mapping::uri(&item.path)?,
                        target_range: mapping::range(&item)?,
                        target_selection_range: mapping::range(&item)?,
                    })
                })
                .collect())
        })
    }
}

impl CompletionProvider for Provider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let task = self.request(Kind::Completion, text, offset, cx);
        cx.spawn(async move |_| {
            let result = task.await?;
            let code::Content::Completions(values) = result.content else {
                return Ok(CompletionResponse::Array(vec![]));
            };
            let values = values
                .into_iter()
                .map(|item| lsp_types::CompletionItem {
                    label: item.label,
                    detail: item.detail,
                    text_edit: item.range.as_ref().and_then(mapping::range).map(|range| {
                        lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range,
                            new_text: item.insert_text.clone(),
                        })
                    }),
                    insert_text: Some(item.insert_text),
                    insert_text_format: Some(lsp_types::InsertTextFormat::PLAIN_TEXT),
                    sort_text: item.sort_text,
                    filter_text: item.filter_text,
                    ..Default::default()
                })
                .collect();
            Ok(CompletionResponse::Array(values))
        })
    }

    fn is_completion_trigger(&self, _: usize, text: &str, _: &mut App) -> bool {
        text.chars().last().is_some_and(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '.' | ':')
        })
    }
}
