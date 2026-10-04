//! Sidebar ancestry records drop intent; Kit remains the sole owner of geometry.
use super::Target;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Hierarchy(pub BTreeMap<Target, Target>);

impl Hierarchy {
    pub fn remove(&mut self, target: Target) {
        let parent = self.0.remove(&target);
        let children: Vec<_> = self
            .0
            .iter()
            .filter_map(|(child, owner)| (*owner == target).then_some(*child))
            .collect();
        let successor = parent.or_else(|| children.first().copied());
        for child in children {
            if Some(child) == successor {
                self.0.remove(&child);
            } else if let Some(successor) = successor {
                self.0.insert(child, successor);
            }
        }
    }

    pub fn attach(&mut self, target: Target, parent: Target) {
        self.remove(target);
        self.0.insert(target, parent);
    }

    pub fn rename(&mut self, from: Target, to: Target) {
        self.map(|target| if target == from { to } else { target });
    }

    pub fn swap(&mut self, from: Target, to: Target) {
        self.map(|target| {
            if target == from {
                to
            } else if target == to {
                from
            } else {
                target
            }
        });
    }

    fn map(&mut self, map: impl Fn(Target) -> Target) {
        self.0 = self
            .0
            .iter()
            .map(|(child, parent)| (map(*child), map(*parent)))
            .collect();
    }
}
