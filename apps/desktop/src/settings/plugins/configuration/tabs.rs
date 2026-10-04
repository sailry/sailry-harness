use super::*;
use gpui_kit::component::tab::{Tab, TabBar};
use sailry_protocol::plugin::settings::Tab as Section;

impl Editor {
    fn sections(&self) -> &[Section] {
        self.info
            .as_ref()
            .and_then(|info| info.settings.as_ref())
            .map(|schema| schema.tabs.as_slice())
            .unwrap_or(&[])
    }

    pub(super) fn tab_bar(&self, cx: &mut Context<Self>) -> Option<TabBar> {
        let sections = self.sections();
        if sections.is_empty() {
            return None;
        }
        let labels = self
            .info
            .as_ref()?
            .settings
            .as_ref()?
            .locales
            .get::<str>(&rust_i18n::locale());
        Some(
            TabBar::new("plugin-settings-tabs")
                .segmented()
                .equal_width()
                .menu(false)
                .w_full()
                .selected_index(self.tab.min(sections.len() - 1))
                .children(sections.iter().map(|section| {
                    let id = section.id.clone();
                    Tab::new()
                        .label(
                            labels
                                .and_then(|labels| labels.get(&section.id))
                                .unwrap_or(&section.title)
                                .clone(),
                        )
                        .disabled(self.pending || self.loading)
                        .debug_selector(move || format!("plugin-settings-tab-{id}"))
                }))
                .on_click(cx.listener(|editor, &index, _, cx| {
                    editor.tab = index;
                    cx.notify();
                })),
        )
    }

    pub(super) fn visible_fields(&self, cx: &App) -> Vec<Entity<Setting>> {
        let sections = self.sections();
        let Some(section) = sections.get(self.tab.min(sections.len().saturating_sub(1))) else {
            return self.fields.clone();
        };
        let enabled = section.enabled.as_ref().is_none_or(|name| {
            self.fields
                .iter()
                .find(|field| field.read(cx).name == *name)
                .is_some_and(|field| {
                    matches!(
                        field.read(cx).value(cx),
                        Ok(fields::Value::Public(Some(serde_json::Value::Bool(true))))
                    )
                })
        });
        let mut fields: Vec<_> = section
            .fields
            .iter()
            .filter(|name| enabled || section.enabled.as_ref() == Some(name))
            .filter_map(|name| {
                self.fields
                    .iter()
                    .find(|field| field.read(cx).name == *name)
                    .cloned()
            })
            .chain(
                self.fields
                    .iter()
                    .filter(|field| {
                        !sections
                            .iter()
                            .any(|section| section.fields.contains(&field.read(cx).name))
                    })
                    .cloned(),
            )
            .collect();
        if self.panel {
            fields.sort_by_key(|field| field.read(cx).boolean());
        }
        fields
    }
}
