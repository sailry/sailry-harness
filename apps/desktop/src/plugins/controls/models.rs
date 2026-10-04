//! Script models reuse the existing native provider picker and its reasoning choices.
use super::*;
use crate::model_picker::{EffortPicked, Picked, Picker};
use gpui_kit::component::popover::Popover;
use sailry_protocol::{Effort, plugin::models::Catalog};

#[derive(Deserialize)]
struct Props {
    label: String,
    placeholder: String,
    catalog: Catalog,
    selected: Option<String>,
    effort: Effort,
    #[serde(default)]
    disabled: bool,
}

struct Field {
    picker: Entity<Picker>,
    selected: Rc<RefCell<Option<String>>>,
    _subscriptions: Vec<Subscription>,
}

pub(super) fn extend(module: HostModule, events: tokio::sync::mpsc::Sender<Value>) -> HostModule {
    let fields = Rc::new(RefCell::new(BTreeMap::<String, Field>::new()));
    module.component("ModelPopup", move |args, _, cx| {
        let Ok(props) = decode(args.props()).and_then(|value| {
            serde_json::from_value::<Props>(value)
                .map_err(|error| HostError::new(error.to_string()))
        }) else {
            return div().into_any_element();
        };
        let id = args.id().to_owned();
        let mut fields = fields.borrow_mut();
        let field = fields.entry(id.clone()).or_insert_with(|| {
            let picker = cx.new(|_| Picker::new());
            let selected = Rc::new(RefCell::new(None::<String>));
            let picked = events.clone();
            let picked_id = id.clone();
            let efforts = events.clone();
            let effort_id = id.clone();
            let effort_model = selected.clone();
            let subscriptions = vec![
                cx.subscribe(&picker, move |_, event: &Picked, _| {
                    let _ = picked.try_send(json!({"id":picked_id,"value":{
                        "model":event.selection.model,"effort":event.effort
                    }}));
                }),
                cx.subscribe(&picker, move |_, event: &EffortPicked, _| {
                    if let Some(model) = effort_model.borrow().as_ref() {
                        let _ = efforts.try_send(json!({"id":effort_id,"value":{
                            "model":model,"effort":event.0
                        }}));
                    }
                }),
            ];
            Field {
                picker,
                selected,
                _subscriptions: subscriptions,
            }
        });
        *field.selected.borrow_mut() = props.selected.clone();
        let catalog = Catalog {
            models: props
                .catalog
                .models
                .into_iter()
                .filter(|model| model.kind == sailry_protocol::plugin::models::Kind::Provider)
                .collect(),
        };
        field.picker.update(cx, |picker, cx| {
            picker.catalog(&catalog, props.selected.as_deref(), cx);
            picker.composer(props.effort, props.disabled, false, cx);
        });
        let title = catalog
            .models
            .iter()
            .find(|model| Some(&model.id) == props.selected.as_ref())
            .map(|model| model.model.clone())
            .unwrap_or(props.placeholder);
        let picker = field.picker.clone();
        div()
            .w_full()
            .debug_selector({
                let id = id.clone();
                move || id.clone()
            })
            .child(
                Popover::new(SharedString::from(id.clone()))
                    .p_2()
                    .anchor(Anchor::TopLeft)
                    .trigger(
                        Button::new(SharedString::from(id))
                            .outline()
                            .w_full()
                            .disabled(props.disabled)
                            .accessibility_label(props.label)
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .text_sm()
                                    .child(div().flex_1().min_w_0().truncate().child(title))
                                    .child(IconName::ChevronDown),
                            ),
                    )
                    .content(move |_, _, cx| {
                        let popover = cx.entity().downgrade();
                        picker.update(cx, |picker, _| picker.popover = Some(popover));
                        picker.clone()
                    }),
            )
            .into_any_element()
    })
}
