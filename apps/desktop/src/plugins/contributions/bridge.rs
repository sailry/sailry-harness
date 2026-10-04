use sailry_protocol::plugin::ui::{self, Contribution, Event, EventKind, State};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, watch};

#[cfg(test)]
mod tests;

pub(crate) struct Bridge {
    pub declarations: Vec<Contribution>,
    current: Mutex<Current>,
    updates: watch::Sender<BTreeMap<String, State>>,
    actions: mpsc::Sender<Event>,
    receiver: tokio::sync::Mutex<mpsc::Receiver<Event>>,
}

struct Current {
    sequence: u64,
    published: bool,
    interactions: BTreeMap<String, Interaction>,
}

struct Interaction {
    sequence: u64,
    busy: bool,
}

impl Bridge {
    pub fn new(declarations: Vec<Contribution>) -> Arc<Self> {
        let values = declarations
            .iter()
            .map(|entry| (entry.id.clone(), State::pending(entry)))
            .collect();
        let (updates, _) = watch::channel(values);
        let (actions, receiver) = mpsc::channel(16);
        Arc::new(Self {
            declarations,
            updates,
            actions,
            receiver: tokio::sync::Mutex::new(receiver),
            current: Mutex::new(Current {
                sequence: 0,
                published: false,
                interactions: BTreeMap::new(),
            }),
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<BTreeMap<String, State>> {
        self.updates.subscribe()
    }

    pub(super) fn state(&self, id: &str) -> Option<State> {
        self.updates.borrow().get(id).cloned()
    }

    pub fn ready(&self) -> bool {
        self.current.lock().is_ok_and(|current| current.published)
    }

    pub fn publish(&self, states: Vec<State>) -> Result<(), &'static str> {
        if states.len() > ui::MAX_ENTRIES
            || serde_json::to_vec(&states)
                .map_err(|_| "invalid contribution state")?
                .len()
                > ui::MAX_STATE_BYTES
        {
            return Err("contribution state exceeds the size limit");
        }
        for (index, state) in states.iter().enumerate() {
            let entry = self
                .declarations
                .iter()
                .find(|entry| entry.id == state.id)
                .ok_or("contribution is not declared")?;
            if !state.valid(entry) || states[..index].iter().any(|before| before.id == state.id) {
                return Err("invalid contribution state");
            }
        }
        let mut current = self
            .current
            .lock()
            .map_err(|_| "contribution state is unavailable")?;
        if states.iter().any(|state| {
            state.reply_to.is_some_and(|reply| {
                reply == 0
                    || reply
                        > current
                            .interactions
                            .get(&state.id)
                            .map_or(0, |interaction| interaction.sequence)
            })
        }) {
            return Err("unknown contribution interaction");
        }
        current.published |= !states.is_empty();
        self.updates.send_modify(|values| {
            for mut state in states {
                if let Some(interaction) = current.interactions.get_mut(&state.id) {
                    if state
                        .reply_to
                        .is_some_and(|reply| reply < interaction.sequence)
                    {
                        continue;
                    }
                    if state.reply_to == Some(interaction.sequence) {
                        interaction.busy = false;
                    }
                    state.enabled &= !interaction.busy;
                }
                values.insert(state.id.clone(), state);
            }
        });
        Ok(())
    }

    pub fn invoke(&self, id: &str, kind: EventKind, value: Value) -> Result<(), &'static str> {
        self.dispatch(id, kind, value, false)
    }

    pub(super) fn invoke_intent(&self, id: &str, value: Value) -> Result<(), &'static str> {
        self.dispatch(id, EventKind::Invoke, value, true)
    }

    fn dispatch(
        &self,
        id: &str,
        kind: EventKind,
        value: Value,
        intent: bool,
    ) -> Result<(), &'static str> {
        let entry = self
            .declarations
            .iter()
            .find(|entry| entry.id == id)
            .ok_or("contribution is not declared")?;
        let handler = entry.handler.clone().ok_or("contribution has no action")?;
        let mut current = self
            .current
            .lock()
            .map_err(|_| "contribution state is unavailable")?;
        let states = self.updates.borrow();
        let state = states
            .get(id)
            .filter(|state| (state.visible || intent && entry.intent.is_some()) && state.enabled);
        let Some(state) = state else {
            return Err("contribution is unavailable");
        };
        let choices = state.choices.as_deref().unwrap_or(&entry.choices);
        let valid = match (entry.kind, kind) {
            (ui::Kind::Button | ui::Kind::Popover, EventKind::Invoke) => {
                value.is_null()
                    || intent
                        && entry.intent.is_some()
                        && serde_json::to_vec(&value)
                            .is_ok_and(|value| value.len() <= ui::MAX_STATE_BYTES)
            }
            (ui::Kind::Toggle | ui::Kind::Popover, EventKind::Change) => value.is_boolean(),
            (ui::Kind::Select | ui::Kind::Menu | ui::Kind::Picker, EventKind::Change) => choices
                .iter()
                .any(|choice| choice.enabled && value.as_str() == Some(&choice.id)),
            (ui::Kind::Picker, EventKind::Search) => {
                value.as_str().is_some_and(|query| query.len() <= 4096)
            }
            _ => false,
        };
        if !valid {
            return Err("invalid contribution interaction");
        }
        drop(states);
        let sequence = current
            .sequence
            .checked_add(1)
            .ok_or("contribution sequence is exhausted")?;
        self.actions
            .try_send(Event {
                sequence,
                id: id.into(),
                handler,
                kind,
                value,
            })
            .map_err(|_| "contribution is busy")?;
        current.sequence = sequence;
        let busy = kind != EventKind::Search;
        current
            .interactions
            .insert(id.into(), Interaction { sequence, busy });
        if busy {
            self.updates.send_modify(|states| {
                if let Some(state) = states.get_mut(id) {
                    state.enabled = false;
                }
            });
        }
        Ok(())
    }

    pub async fn next(&self) -> Option<Event> {
        self.receiver.lock().await.recv().await
    }
}
