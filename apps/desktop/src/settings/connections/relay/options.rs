//! Service choices retain exact destinations while catalog metadata and timings change.
use super::*;
use gpui_kit::component::{
    searchable_list::{SearchableListDelegate, SearchableListItem},
    spinner::Spinner,
    tooltip::Tooltip,
};
use sailry_protocol::{network::Preset, plugin::desktop::Navigation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Preset { id: String, selection: Selection },
    Retained(Selection),
    Custom,
}

impl Choice {
    pub(super) fn selection(&self) -> Option<Selection> {
        match self {
            Self::Preset { selection, .. } | Self::Retained(selection) => Some(selection.clone()),
            Self::Custom => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Timing {
    Unknown,
    Pending,
    Measured(Option<u64>),
}

#[derive(Clone)]
pub(super) struct Item {
    pub choice: Choice,
    label: Option<Navigation>,
    pub timing: Timing,
}

impl SearchableListItem for Item {
    type Value = Choice;

    fn title(&self) -> SharedString {
        self.label.as_ref().map_or_else(
            || {
                tr(if self.choice == Choice::Custom {
                    "pairing_relay_custom"
                } else {
                    "pairing_relay_selected"
                })
            },
            |label| label.label(&rust_i18n::locale()).to_owned().into(),
        )
    }

    fn value(&self) -> &Choice {
        &self.choice
    }
}

pub(super) struct Options(pub Vec<Item>);

impl SearchableListDelegate for Options {
    type Item = Item;

    fn items_count(&self, _: usize) -> usize {
        self.0.len()
    }

    fn item(&self, index: IndexPath) -> Option<&Item> {
        self.0.get(index.row)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.0
            .iter()
            .position(|item| item.value() == value)
            .map(IndexPath::new)
    }

    fn render_item(
        &self,
        _: IndexPath,
        item: &Item,
        checked: bool,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let timing = item.timing;
        Some(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(div().flex_1().min_w_0().truncate().child(item.title()))
                .child(
                    div()
                        .id("relay-latency")
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .tooltip(|window, cx| {
                            Tooltip::new(tr("pairing_relay_latency_hint")).build(window, cx)
                        })
                        .child(match timing {
                            Timing::Pending => Spinner::new().small().into_any_element(),
                            _ => div().child(timing.text()).into_any_element(),
                        }),
                )
                .child(
                    Icon::new(IconName::Check)
                        .xsmall()
                        .when(!checked, |icon| icon.invisible()),
                )
                .into_any_element(),
        )
    }
}

impl Timing {
    fn text(self) -> SharedString {
        match self {
            Self::Measured(Some(millis)) => {
                rust_i18n::t!("usage_duration_ms", value = millis).into()
            }
            _ => tr("composer_metric_unknown"),
        }
    }
}

pub(super) fn initial(selection: Option<&Selection>) -> Vec<Item> {
    selection
        .map(|selection| Item {
            choice: Choice::Retained(selection.clone()),
            label: None,
            timing: Timing::Unknown,
        })
        .into_iter()
        .chain([custom()])
        .collect()
}

fn custom() -> Item {
    Item {
        choice: Choice::Custom,
        label: None,
        timing: Timing::Unknown,
    }
}

pub(super) fn retain(items: &[Item], selection: Option<&Selection>) -> (Vec<Item>, Choice) {
    let selected = selection.and_then(|selection| {
        items
            .iter()
            .find(|item| item.choice.selection().as_ref() == Some(selection))
    });
    let mut next = items
        .iter()
        .filter(|item| {
            !matches!(item.choice, Choice::Retained(_))
                || item.choice.selection().as_ref() == selection
        })
        .cloned()
        .collect::<Vec<_>>();
    if let Some(selected) = selected {
        return (next, selected.choice.clone());
    }
    let choice = selection.map_or(Choice::Custom, |selection| {
        Choice::Retained(selection.clone())
    });
    if choice != Choice::Custom {
        next.insert(
            0,
            Item {
                choice: choice.clone(),
                label: None,
                timing: Timing::Unknown,
            },
        );
    }
    (next, choice)
}

pub(super) fn merge(
    presets: Vec<Preset>,
    previous: &[Item],
    selected: Option<&Choice>,
) -> (Vec<Item>, Option<Choice>) {
    let mut items = presets
        .into_iter()
        .map(|preset| {
            let timing = previous
                .iter()
                .find(|item| item.choice.selection().as_ref() == Some(&preset.selection))
                .map_or(Timing::Unknown, |item| item.timing);
            Item {
                choice: Choice::Preset {
                    id: preset.id,
                    selection: preset.selection,
                },
                label: Some(preset.label),
                timing,
            }
        })
        .collect::<Vec<_>>();
    let (mut items, selected) = if selected == Some(&Choice::Custom) {
        (items, Some(Choice::Custom))
    } else if let Some(selected) = selected
        && let Some(choice) = selected.selection()
    {
        let timing = previous
            .iter()
            .find(|item| item.choice.selection().as_ref() == Some(&choice))
            .map_or(Timing::Unknown, |item| item.timing);
        let selected = match selected {
            Choice::Preset { .. } if items.iter().any(|item| item.choice == *selected) => {
                selected.clone()
            }
            Choice::Preset { .. } => {
                let retained = Choice::Retained(choice);
                items.insert(
                    0,
                    Item {
                        choice: retained.clone(),
                        label: None,
                        timing,
                    },
                );
                retained
            }
            _ => {
                let (next, selected) = retain(&items, Some(&choice));
                items = next;
                selected
            }
        };
        if let Some(item) = items.iter_mut().find(|item| item.choice == selected) {
            item.timing = timing;
        }
        (items, Some(selected))
    } else {
        (items, None)
    };
    items.push(
        previous
            .iter()
            .find(|item| item.choice == Choice::Custom)
            .cloned()
            .unwrap_or_else(custom),
    );
    (items, selected)
}

pub(super) fn selection(
    choice: Option<&Choice>,
    address: &str,
    saved: Option<&Selection>,
) -> Option<Selection> {
    let choice = choice?;
    if let Some(selection) = choice.selection() {
        return Some(selection);
    }
    let address = address.trim();
    if address.is_empty() {
        return None;
    }
    if let Some(Selection::Custom { relays }) = saved
        && relays.first().is_some_and(|first| first == address)
    {
        return Some(Selection::Custom {
            relays: relays.clone(),
        });
    }
    Some(Selection::Custom {
        relays: vec![address.to_owned()],
    })
}

#[cfg(test)]
mod tests;
