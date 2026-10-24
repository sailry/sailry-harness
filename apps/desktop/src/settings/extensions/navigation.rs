//! Settings declarations join the core menu without owning another page runtime.
use crate::settings::Section;
use sailry_protocol::{
    NodeId,
    plugin::{
        Info, Reference, Summary,
        desktop::{Navigation, SettingsGroup, SettingsPlacement},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub node: NodeId,
    pub package: Reference,
    pub navigation: Navigation,
    pub placement: SettingsPlacement,
}

impl Entry {
    pub fn new(node: NodeId, info: &Info, current: &Summary) -> Option<Self> {
        if info.summary.name != current.name || info.summary.digest != current.digest {
            return None;
        }
        let page = info.extension.as_ref()?.settings_page.as_ref()?;
        Some(Self {
            node,
            package: current.reference(),
            navigation: page.navigation.clone(),
            placement: page.placement,
        })
    }

    pub fn available(&self, node: NodeId, summaries: &[Summary]) -> bool {
        self.node == node
            && summaries
                .iter()
                .any(|summary| summary.reference() == self.package)
    }
}

pub(crate) enum Item {
    Core(Section),
    Plugin(Entry),
}

impl Item {
    fn order(&self) -> u16 {
        match self {
            Self::Core(section) => section.navigation_order(),
            Self::Plugin(entry) => entry.placement.order,
        }
    }

    pub fn selected(&self, section: Section, plugin: Option<&str>) -> bool {
        match self {
            Self::Core(expected) => section == *expected,
            Self::Plugin(entry) => {
                section == Section::Plugin && plugin == Some(entry.package.name.as_str())
            }
        }
    }
}

pub(crate) fn group(group: SettingsGroup, core: &[Section], pages: &[Entry]) -> Vec<Item> {
    let mut items: Vec<_> = core
        .iter()
        .copied()
        .map(Item::Core)
        .chain(
            pages
                .iter()
                .filter(|entry| entry.placement.group == group)
                .cloned()
                .map(Item::Plugin),
        )
        .collect();
    items.sort_by_key(Item::order);
    items
}

#[cfg(test)]
mod tests;
