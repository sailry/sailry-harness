//! Presentation and drafts for the Node's iroh relay selection.
use super::*;
use gpui_kit::component::{form::Field, searchable_list::SearchableListItem};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Network {
    Iroh,
    Custom,
}

impl SearchableListItem for Network {
    type Value = Self;
    fn title(&self) -> SharedString {
        tr(match self {
            Self::Iroh => "pairing_relay_iroh",
            Self::Custom => "pairing_relay_custom",
        })
    }
    fn value(&self) -> &Self {
        self
    }
}

pub(super) fn fields(
    window: &mut Window,
    cx: &mut Context<Connections>,
) -> (Entity<SelectState<Vec<Network>>>, Entity<InputState>) {
    let selection = cx
        .try_global::<Services>()
        .and_then(|services| services.link.relay_selection().ok().flatten());
    let address = match &selection {
        Some(RelaySelection::Custom(urls)) => urls.first().cloned().unwrap_or_default(),
        _ => String::new(),
    };
    let custom = matches!(selection, Some(RelaySelection::Custom(_)));
    let select = cx.new(|cx| {
        SelectState::new(
            vec![Network::Iroh, Network::Custom],
            Some(IndexPath::new(usize::from(custom))),
            window,
            cx,
        )
    });
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(tr("pairing_relay_address_hint"))
            .default_value(address)
    });
    cx.subscribe(&select, |view, _, event, cx| {
        if matches!(event, SelectEvent::Confirm(_)) && view.sharing {
            view.share(cx);
        }
    })
    .detach();
    cx.subscribe(&input, |view, _, event, cx| {
        if matches!(event, InputEvent::Change) && view.sharing && view.custom_relay(cx) {
            view.start_share(Duration::from_millis(650), cx);
        }
    })
    .detach();
    (select, input)
}

impl Connections {
    pub(super) fn custom_relay(&self, cx: &App) -> bool {
        self.relay_select.read(cx).selected_value() == Some(&Network::Custom)
    }

    pub(super) fn relay_selection(&self, cx: &App) -> Option<RelaySelection> {
        if !self.custom_relay(cx) {
            return Some(RelaySelection::Default);
        }
        let address = self.relay_address.read(cx).value().trim().to_string();
        (!address.is_empty()).then_some(RelaySelection::Custom(vec![address]))
    }

    pub(super) fn relay_fields(&self, cx: &App) -> Div {
        v_flex()
            .w_full()
            .gap_4()
            .child(
                Field::new().label_indent(false).child(
                    div()
                        .w_full()
                        .debug_selector(|| "pairing-relay-select".into())
                        .child(
                            Select::new(&self.relay_select)
                                .w_full()
                                .accessibility_label(tr("pairing_relay_server")),
                        ),
                ),
            )
            .when(self.custom_relay(cx), |body| {
                body.child(
                    Field::new().label(tr("pairing_relay_address")).child(
                        div()
                            .w_full()
                            .debug_selector(|| "pairing-relay-address".into())
                            .child(
                                Input::new(&self.relay_address)
                                    .w_full()
                                    .content_type(gpui_kit::component::input::InputContentType::Url)
                                    .aria_label(tr("pairing_relay_address")),
                            ),
                    ),
                )
            })
    }
}
