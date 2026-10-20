//! Shared provider groups for conversation and plugin model selection.
use crate::{
    settings::{Channel, ModelCategory},
    tr,
};
use gpui_kit::component::popover::PopoverState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::BTreeSet;

pub(crate) fn panel_size(window: &Window) -> Size<Pixels> {
    // Include the Kit popover's p_3 padding in the requested outer width.
    let padding = window.rem_size() * 1.5;
    size(
        (px(320.) - padding).min(window.viewport_size().width - px(72.)),
        px(320.)
            .min(window.viewport_size().height * 0.5)
            .max(px(268.)),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub channel: usize,
    pub model: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    id: usize,
    name: String,
    category: ModelCategory,
    family: String,
    directory: Option<sailry_protocol::conversation::presets::Entry>,
    models: Vec<Choice>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice {
    id: String,
    label: String,
    effort: sailry_protocol::Effort,
    efforts: Vec<sailry_protocol::Effort>,
}

impl From<Channel> for Group {
    fn from(channel: Channel) -> Self {
        let directory = channel
            .directory
            .or_else(|| crate::settings::fallback_preset(channel.preset));
        Group {
            id: channel.id,
            name: channel.name,
            category: channel.preset.category(),
            family: directory.as_ref().map_or_else(
                || channel.preset.category().key().to_owned(),
                crate::settings::type_key,
            ),
            directory,
            models: channel
                .models
                .into_iter()
                .map(|model| Choice {
                    efforts: if model.reasoning {
                        crate::reasoning::choices(&model.efforts)
                    } else {
                        vec![]
                    },
                    label: model.label().to_owned(),
                    id: model.id,
                    effort: sailry_protocol::Effort::initial(&model.efforts, model.default_effort),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
mod view;

pub(crate) struct Picker {
    channels: Vec<Group>,
    selected: Option<Selection>,
    pub popover: Option<WeakEntity<PopoverState>>,
    family: Option<String>,
    categories: bool,
    collapsed: BTreeSet<usize>,
    scroll: ScrollHandle,
    current_effort: Option<sailry_protocol::Effort>,
    disabled: bool,
}

pub(crate) struct Picked {
    pub selection: Selection,
    pub effort: sailry_protocol::Effort,
}

impl EventEmitter<Picked> for Picker {}

pub(crate) struct Back;
pub(crate) struct EffortPicked(pub sailry_protocol::Effort);
impl EventEmitter<Back> for Picker {}
impl EventEmitter<EffortPicked> for Picker {}

impl Picker {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            selected: None,
            popover: None,
            family: None,
            categories: true,
            collapsed: BTreeSet::new(),
            scroll: ScrollHandle::new(),
            current_effort: None,
            disabled: false,
        }
    }

    pub fn set(
        &mut self,
        channels: Vec<Channel>,
        selected: Option<Selection>,
        cx: &mut Context<Self>,
    ) {
        let channels = channels
            .into_iter()
            .filter(|channel| channel.enabled)
            .map(Group::from)
            .collect();
        self.set_groups(channels, selected, cx);
    }

    pub fn catalog(
        &mut self,
        catalog: &sailry_protocol::plugin::models::Catalog,
        selected: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let mut groups: Vec<Group> = Vec::new();
        let mut providers = Vec::new();
        for model in &catalog.models {
            let provider = model
                .id
                .split_once('/')
                .map(|(provider, _)| provider)
                .unwrap_or(&model.provider);
            let id = match providers.iter().position(|value| *value == provider) {
                Some(index) => index,
                None => {
                    let index = groups.len();
                    providers.push(provider);
                    groups.push(Group {
                        id: index,
                        name: model.provider.clone(),
                        category: ModelCategory::Other,
                        family: ModelCategory::Other.key().to_owned(),
                        directory: None,
                        models: Vec::new(),
                    });
                    index
                }
            };
            groups[id].models.push(Choice {
                efforts: crate::reasoning::choices(&model.efforts),
                id: model.id.clone(),
                label: model.model.clone(),
                effort: sailry_protocol::Effort::initial(&model.efforts, model.default_effort),
            });
        }
        let selection = groups.iter().find_map(|group| {
            group
                .models
                .iter()
                .find(|model| Some(model.id.as_str()) == selected)
                .map(|model| Selection {
                    channel: group.id,
                    model: model.id.clone(),
                })
        });
        self.set_groups(groups, selection, cx);
        self.family = None;
        self.categories = false;
    }

    fn set_groups(
        &mut self,
        channels: Vec<Group>,
        selected: Option<Selection>,
        cx: &mut Context<Self>,
    ) {
        if self.channels != channels || self.selected != selected {
            let changed = self.selected != selected
                || self
                    .selected
                    .as_ref()
                    .and_then(|selection| {
                        self.channels
                            .iter()
                            .find(|channel| channel.id == selection.channel)
                    })
                    .map(|channel| channel.family.clone())
                    != selected
                        .as_ref()
                        .and_then(|selection| {
                            channels
                                .iter()
                                .find(|channel| channel.id == selection.channel)
                        })
                        .map(|channel| channel.family.clone());
            self.channels = channels;
            self.selected = selected;
            let missing = self
                .family
                .as_ref()
                .is_some_and(|family| !self.has_family(family));
            if missing {
                self.family = None;
            }
            if changed || missing {
                self.reveal(cx);
            }
            cx.notify();
        }
    }

    pub fn composer(
        &mut self,
        effort: sailry_protocol::Effort,
        disabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.current_effort != Some(effort) || self.disabled != disabled {
            self.current_effort = Some(effort);
            self.disabled = disabled;
            cx.notify();
        }
    }

    fn pick(
        &mut self,
        selection: Selection,
        effort: sailry_protocol::Effort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        cx.emit(Picked { selection, effort });
        if let Some(popover) = &self.popover {
            _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
        }
    }

    pub fn reveal(&mut self, cx: &mut Context<Self>) {
        if let Some(channel) = self.selected.as_ref().and_then(|selection| {
            self.channels
                .iter()
                .find(|channel| channel.id == selection.channel && !channel.models.is_empty())
        }) {
            self.family = Some(channel.family.clone());
            self.collapsed.remove(&channel.id);
        }
        cx.notify();
    }

    fn has_family(&self, family: &str) -> bool {
        self.channels
            .iter()
            .any(|channel| channel.family == family && !channel.models.is_empty())
    }
}
