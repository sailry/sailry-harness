//! Kit's script bindings do not expose Sailry's shared identity picker.
//! This adapter retains the existing native popover, swatches and icon buttons.
use super::*;
use sailry_protocol::projects::{Appearance, COLORS, ICONS};

#[derive(Deserialize)]
struct Props {
    value: Appearance,
    #[serde(default)]
    label: String,
    #[serde(default)]
    disabled: bool,
}

struct State {
    id: String,
    value: Appearance,
    projected: Appearance,
    disabled: bool,
    sender: tokio::sync::mpsc::Sender<Value>,
    stop: CancellationToken,
}

fn props(value: &HostValue) -> Option<Props> {
    let props: Props = serde_json::from_value(decode(value).ok()?).ok()?;
    (ICONS.contains(&props.value.icon.as_str()) && COLORS.contains(&props.value.color.as_str()))
        .then_some(props)
}

pub(super) fn extend(
    module: HostModule,
    sender: tokio::sync::mpsc::Sender<Value>,
    stop: CancellationToken,
) -> HostModule {
    let states = Rc::new(RefCell::new(BTreeMap::<String, Entity<State>>::new()));
    module
        .component("Appearance", |args, _, cx| {
            props(args.props()).map_or_else(
                || div().into_any_element(),
                |props| crate::workspace::appearance::project(&props.value, cx).into_any_element(),
            )
        })
        .component("AppearancePicker", move |args, _, cx| {
            let Some(props) = props(args.props()) else {
                return div().into_any_element();
            };
            let id = args.id().to_owned();
            let owner = states
                .borrow_mut()
                .entry(id.clone())
                .or_insert_with(|| {
                    cx.new(|_| State {
                        id: id.clone(),
                        value: props.value.clone(),
                        projected: props.value.clone(),
                        disabled: props.disabled,
                        sender: sender.clone(),
                        stop: stop.clone(),
                    })
                })
                .clone();
            owner.update(cx, |state, _| {
                if state.projected != props.value {
                    state.value = props.value.clone();
                    state.projected = props.value.clone();
                }
                state.disabled = props.disabled;
            });
            crate::workspace::appearance::picker(
                &id,
                owner.downgrade(),
                crate::workspace::appearance::Selection {
                    selected: owner.read(cx).value.clone(),
                    read: |state: &State| state.value.clone(),
                    write: |state, value| {
                        if state.disabled || state.stop.is_cancelled() {
                            return;
                        }
                        state.value = value.clone();
                        let _ = state.sender.try_send(json!({"id":state.id,"value":value}));
                    },
                },
                props.disabled || stop.is_cancelled(),
                props.label.into(),
                cx,
            )
            .into_any_element()
        })
}
