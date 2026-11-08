use super::*;
use sailry_client::Client;
use sailry_protocol::{Effort, NodeId, ProviderId, conversation::catalog};
use std::{collections::BTreeMap, sync::Arc};

mod cache;
mod ring;
mod view;
pub(in crate::model_picker) use view::effort_content;
pub(crate) use view::effort_item;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct Binding {
    pub client: Arc<Client>,
    pub runtime: tokio::runtime::Handle,
    pub providers: BTreeMap<usize, ProviderId>,
}

struct Bound {
    host: Entity<cache::Host>,
    providers: BTreeMap<usize, ProviderId>,
    _observer: Subscription,
}

#[derive(Default)]
pub(super) struct State {
    sources: BTreeMap<NodeId, Bound>,
    pub(super) requested: BTreeMap<NodeId, (u64, Vec<catalog::ModelRef>)>,
    preview: bool,
}

#[derive(Clone)]
pub(super) struct Measured {
    score: catalog::EffortScore,
    preview: bool,
}

fn rank(effort: Effort) -> Option<u8> {
    match effort {
        Effort::Disabled => Some(0),
        Effort::Minimal => Some(1),
        Effort::Low => Some(2),
        Effort::Medium => Some(3),
        Effort::High => Some(4),
        Effort::XHigh => Some(5),
        Effort::Max => Some(6),
        Effort::Default | Effort::Budget(_) => None,
    }
}

pub(super) fn effective(selected: bool, current: Option<Effort>, initial: Effort) -> Effort {
    if selected {
        current.unwrap_or(initial)
    } else {
        initial
    }
}

fn nearest(scores: &[catalog::EffortScore], effort: Effort) -> Option<&catalog::EffortScore> {
    if let Some(exact) = scores.iter().find(|score| score.effort == effort) {
        return Some(exact);
    }
    let requested = rank(effort);
    scores
        .iter()
        .filter_map(|score| rank(score.effort).map(|rank| (score, rank)))
        .filter(|(_, rank)| requested.is_some_and(|requested| *rank <= requested))
        .max_by_key(|(_, rank)| *rank)
        .or_else(|| {
            scores
                .iter()
                .filter_map(|score| rank(score.effort).map(|rank| (score, rank)))
                .min_by_key(|(_, rank)| *rank)
        })
        .map(|(score, _)| score)
        .or_else(|| scores.iter().find(|score| score.effort == Effort::Default))
}

impl Bound {
    fn reference(&self, group: &Group, model: &Choice) -> Option<catalog::ModelRef> {
        model.reference.clone().or_else(|| {
            Some(catalog::ModelRef {
                provider: *self.providers.get(&group.id)?,
                model: model.id.clone(),
            })
        })
    }
}

impl Picker {
    pub fn bind_scores(&mut self, bindings: Vec<Binding>, cx: &mut Context<Self>) {
        self.scores.preview = false;
        let nodes = bindings
            .iter()
            .map(|binding| binding.client.target())
            .collect::<BTreeSet<_>>();
        self.scores.sources.retain(|node, _| nodes.contains(node));
        self.scores.requested.retain(|node, _| nodes.contains(node));
        for binding in bindings {
            let node = binding.client.target();
            if let Some(bound) = self.scores.sources.get_mut(&node) {
                if bound.providers != binding.providers {
                    bound.providers = binding.providers;
                    self.scores.requested.remove(&node);
                }
            } else {
                let host = cache::get(&binding, cx);
                let observer = cx.observe(&host, |_, _, cx| cx.notify());
                self.scores.sources.insert(
                    node,
                    Bound {
                        host,
                        providers: binding.providers,
                        _observer: observer,
                    },
                );
            }
        }
    }

    pub fn preview_scores(&mut self) {
        self.scores.preview = true;
        self.scores.sources.clear();
        self.scores.requested.clear();
    }

    pub(super) fn request_scores(&mut self, groups: &[Group], cx: &mut Context<Self>) {
        for (node, bound) in &self.scores.sources {
            let references = groups
                .iter()
                .flat_map(|group| {
                    group
                        .models
                        .iter()
                        .filter_map(|model| bound.reference(group, model))
                })
                .collect::<Vec<_>>();
            let status = &bound.host.read(cx).cache.status;
            let request = (status.revision, references.clone());
            if self.scores.requested.get(node) == Some(&request) {
                continue;
            }
            self.scores.requested.insert(*node, request);
            bound
                .host
                .update(cx, |host, cx| host.request(references, cx));
        }
    }

    pub fn request_selected_score(&mut self, cx: &mut Context<Self>) {
        let Some((group, model)) = self.selected_choice() else {
            return;
        };
        let mut group = group.clone();
        group.models = vec![model.clone()];
        self.request_scores(&[group], cx);
    }

    pub(super) fn measured(
        &self,
        group: &Group,
        model: &Choice,
        effort: Effort,
        cx: &App,
    ) -> Option<Measured> {
        if self.scores.preview {
            return Some(Measured {
                score: catalog::EffortScore {
                    effort,
                    overall: 82.4,
                    axes: [81, 87, 85, 79, 80],
                    elapsed_ms: Some(7_200_000),
                    cost_usd: Some(12.5),
                },
                preview: true,
            });
        }
        self.scores.sources.values().find_map(|bound| {
            let reference = bound.reference(group, model)?;
            let host = bound.host.read(cx);
            let scores = host.scores(&reference)?;
            Some(Measured {
                // An explicit effort shows only its own measurement; the
                // provider default borrows the nearest measured effort.
                score: if effort == Effort::Default {
                    nearest(scores, effort)?
                } else {
                    scores.iter().find(|score| score.effort == effort)?
                }
                .clone(),
                preview: false,
            })
        })
    }

    pub(super) fn exact(
        &self,
        group: &Group,
        model: &Choice,
        effort: Effort,
        cx: &App,
    ) -> Option<f32> {
        let measured = self.measured(group, model, effort, cx)?;
        (measured.score.effort == effort).then_some(measured.score.overall)
    }

    pub fn effort_score(&self, effort: Effort, cx: &App) -> Option<f32> {
        let (group, model) = self.selected_choice()?;
        self.exact(group, model, effort, cx)
    }

    pub fn has_scores(&self, cx: &App) -> bool {
        self.selected_choice()
            .and_then(|(group, model)| self.measured(group, model, model.effort, cx))
            .is_some()
    }

    fn selected_choice(&self) -> Option<(&Group, &Choice)> {
        self.choice(self.selected.as_ref()?)
    }
}
