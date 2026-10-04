//! Kit b79f4ce's script Select cannot receive a selected value after construction.
//! Keep searchable native Select states while package snapshots control selection.
use super::*;
use gpui_kit::component::{
    searchable_list::{SearchableListItem, SearchableVec},
    select::{Select, SelectEvent, SelectState},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Deserialize, PartialEq)]
struct Item {
    id: String,
    label: String,
}
impl SearchableListItem for Item {
    type Value = String;
    fn title(&self) -> SharedString {
        self.label.clone().into()
    }
    fn value(&self) -> &String {
        &self.id
    }
}

#[derive(Deserialize)]
struct Props {
    label: String,
    items: Vec<Item>,
    selected: Option<String>,
    placeholder: Option<String>,
    #[serde(default)]
    disabled: bool,
}

struct Field {
    state: Entity<SelectState<SearchableVec<Item>>>,
    items: Vec<Item>,
    selected: Option<String>,
    disabled: bool,
    placeholder: Option<String>,
    _subscription: Subscription,
}

pub(super) fn extend(module: HostModule, events: tokio::sync::mpsc::Sender<Value>) -> HostModule {
    let fields = Rc::new(RefCell::new(BTreeMap::<String, Field>::new()));
    module.component("SelectField", move |args, window, cx| {
        let Ok(props) = decode(args.props()).and_then(|value| {
            serde_json::from_value::<Props>(value)
                .map_err(|error| HostError::new(error.to_string()))
        }) else {
            return div().into_any_element();
        };
        let id = args.id().to_owned();
        let mut fields = fields.borrow_mut();
        let field = fields.entry(id.clone()).or_insert_with(|| {
            let state = cx.new(|cx| {
                SelectState::new(SearchableVec::new(props.items.clone()), None, window, cx)
                    .searchable(true)
            });
            let events = events.clone();
            let id = id.clone();
            let subscription = cx.subscribe(&state, move |_, event, _| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    let _ = events.try_send(json!({"id":id,"value":value}));
                }
            });
            Field {
                state,
                items: vec![],
                selected: None,
                disabled: false,
                placeholder: None,
                _subscription: subscription,
            }
        });
        let changed = field.items != props.items || field.selected != props.selected;
        let appearance = field.disabled != props.disabled || field.placeholder != props.placeholder;
        field.disabled = props.disabled;
        field.placeholder = props.placeholder.clone();
        if field.items != props.items {
            field.items = props.items.clone();
            field.state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(props.items), window, cx)
            });
        }
        if changed {
            field.selected = props.selected.clone();
            field.state.update(cx, |state, cx| {
                if let Some(selected) = &props.selected {
                    state.set_selected_value(selected, window, cx);
                } else {
                    state.set_selected_index(None, window, cx);
                }
            });
        }
        if changed || appearance {
            field.state.update(cx, |_, cx| cx.notify());
        }
        div()
            .w_full()
            .debug_selector(move || id.clone())
            .child(
                Select::new(&field.state)
                    .w_full()
                    .menu_width(Length::Auto)
                    .disabled(props.disabled)
                    .accessibility_label(props.label)
                    .when_some(props.placeholder, |field, placeholder| {
                        field.placeholder(placeholder)
                    }),
            )
            .into_any_element()
    })
}
