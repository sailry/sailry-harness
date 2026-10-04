use super::*;

pub(crate) struct Tab {
    pub id: SessionId,
    pub view: Entity<View>,
    pub _observer: Subscription,
}

pub(crate) struct Panel {
    pub source: Entity<View>,
    pub selected: SessionId,
    pub tabs: Vec<Tab>,
    pub scroll: ScrollHandle,
    pub _observer: Subscription,
}

impl Panel {
    pub(crate) fn child(&self) -> &Entity<View> {
        &self
            .tabs
            .iter()
            .find(|tab| tab.id == self.selected)
            .unwrap()
            .view
    }

    pub(crate) fn select(&mut self, id: SessionId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        self.selected = id;
        self.scroll.scroll_to_item(index);
        true
    }

    pub(crate) fn close(&mut self, id: SessionId) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        if id == self.selected && !self.tabs.is_empty() {
            self.select(self.tabs[index.min(self.tabs.len() - 1)].id);
        }
    }
}

mod view;
