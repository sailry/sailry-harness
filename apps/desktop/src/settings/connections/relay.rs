//! Controller drafts and finite observations of the addressed Node's relay services.
use super::*;
use gpui_kit::component::form::Field;
use sailry_client::Client;
use sailry_protocol::network::{Listing, Selection};
use std::{sync::Arc, time::Duration};

mod load;
mod options;
use options::{Choice, Item, Options, Timing};

pub(super) enum Change {
    Selection,
    Address,
}

pub(super) struct Picker {
    select: Entity<SelectState<Options>>,
    address: Entity<InputState>,
    items: Vec<Item>,
    saved: Option<Selection>,
    binding: Option<(Arc<Client>, Arc<tokio::runtime::Runtime>)>,
    read: Option<load::Read>,
    custom: Option<load::Read>,
    revision: u64,
    custom_revision: u64,
}

impl EventEmitter<Change> for Picker {}

impl Picker {
    pub(super) fn new(
        selection: Option<RelaySelection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = selection.map(super::edit::desired);
        let items = options::initial(saved.as_ref());
        let address = match &saved {
            Some(Selection::Custom { relays }) => relays.first().cloned().unwrap_or_default(),
            _ => String::new(),
        };
        let select = cx.new(|cx| {
            SelectState::new(Options(items.clone()), Some(IndexPath::new(0)), window, cx)
        });
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("pairing_relay_address_hint"))
                .default_value(address)
        });
        cx.subscribe_in(&select, window, |picker, _, event, window, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                picker.probe_custom(window, cx);
                cx.emit(Change::Selection);
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(&address, window, |picker, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                picker.probe_custom(window, cx);
                cx.emit(Change::Address);
                cx.notify();
            }
        })
        .detach();
        Self {
            select,
            address,
            items,
            saved,
            binding: None,
            read: None,
            custom: None,
            revision: 0,
            custom_revision: 0,
        }
    }

    pub(super) fn bind(
        &mut self,
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close();
        self.binding = Some((client.clone(), runtime.clone()));
        for item in &mut self.items {
            item.timing = Timing::Unknown;
        }
        self.sync(window, cx);
        let selected = self.selection(cx);
        self.read = Some(load::catalog(
            client,
            runtime,
            selected,
            self.revision,
            window,
            cx,
        ));
        self.probe_custom(window, cx);
    }

    pub(super) fn close(&mut self) {
        self.revision += 1;
        self.custom_revision += 1;
        self.read = None;
        self.custom = None;
        self.binding = None;
    }

    pub(super) fn set_selection(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saved = Some(selection.clone());
        let (items, selected) = options::retain(&self.items, Some(&selection));
        self.items = items;
        self.sync(window, cx);
        self.select.update(cx, |select, cx| {
            select.set_selected_value(&selected, window, cx);
        });
        let address = match selection {
            Selection::Iroh => String::new(),
            Selection::Custom { relays } => relays.first().cloned().unwrap_or_default(),
        };
        self.address
            .update(cx, |input, cx| input.set_value(address, window, cx));
        cx.notify();
    }

    pub(super) fn custom(&self, cx: &App) -> bool {
        self.select.read(cx).selected_value() == Some(&Choice::Custom)
    }

    pub(super) fn selection(&self, cx: &App) -> Option<RelaySelection> {
        options::selection(
            self.select.read(cx).selected_value(),
            self.address.read(cx).value().as_str(),
            self.saved.as_ref(),
        )
        .map(|selection| super::edit::transport(&selection))
    }

    fn sync(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.select.update(cx, |select, cx| {
            select.set_items(Options(self.items.clone()), window, cx)
        });
        cx.notify();
    }

    fn accept(&mut self, listing: Listing, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.select.read(cx).selected_value().cloned();
        let (items, selected) = options::merge(listing.presets, &self.items, selected.as_ref());
        self.items = items;
        self.sync(window, cx);
        if let Some(selected) = selected {
            self.select.update(cx, |select, cx| {
                select.set_selected_value(&selected, window, cx)
            });
        }
    }

    fn timing(
        &mut self,
        selection: &Selection,
        timing: Timing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for item in &mut self.items {
            if item.choice.selection().as_ref() == Some(selection) {
                item.timing = timing;
            }
        }
        self.sync(window, cx);
    }

    fn probe_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.custom = None;
        self.custom_revision += 1;
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.choice == Choice::Custom)
        {
            item.timing = Timing::Unknown;
        }
        self.sync(window, cx);
        if !self.custom(cx) {
            return;
        }
        let Some(selection) = self.selection(cx).filter(|value| value.validate().is_ok()) else {
            return;
        };
        let Some((client, runtime)) = self.binding.clone() else {
            return;
        };
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.choice == Choice::Custom)
        {
            item.timing = Timing::Pending;
        }
        self.sync(window, cx);
        self.custom = Some(load::custom(
            client,
            runtime,
            super::edit::desired(selection),
            Duration::from_millis(650),
            self.custom_revision,
            window,
            cx,
        ));
    }
}

pub(super) fn controls(picker: &Entity<Picker>, disabled: bool, cx: &App) -> Div {
    let picker = picker.read(cx);
    v_flex()
        .w_full()
        .gap_4()
        .child(
            Field::new().label_indent(false).child(
                div()
                    .w_full()
                    .debug_selector(|| "pairing-relay-select".into())
                    .child(
                        Select::new(&picker.select)
                            .w_full()
                            .disabled(disabled)
                            .accessibility_label(tr("pairing_relay_server")),
                    ),
            ),
        )
        .when(picker.custom(cx), |body| {
            body.child(
                Field::new().label(tr("pairing_relay_address")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "pairing-relay-address".into())
                        .child(
                            Input::new(&picker.address)
                                .w_full()
                                .disabled(disabled)
                                .content_type(gpui_kit::component::input::InputContentType::Url)
                                .aria_label(tr("pairing_relay_address")),
                        ),
                ),
            )
        })
}

impl Connections {
    pub(super) fn custom_relay(&self, cx: &App) -> bool {
        self.relay.read(cx).custom(cx)
    }

    pub(super) fn relay_selection(&self, cx: &App) -> Option<RelaySelection> {
        self.relay.read(cx).selection(cx)
    }

    pub(super) fn relay_fields(&self, cx: &App) -> Div {
        controls(&self.relay, false, cx)
    }

    pub(crate) fn prepare_network(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(services) = cx.try_global::<Services>().cloned() else {
            return;
        };
        self.relay.update(cx, |picker, cx| {
            picker.bind(
                Arc::new(Client::new(services.local)),
                services.runtime,
                window,
                cx,
            )
        });
    }
}
