use super::*;
use gpui_kit::base::actions::Confirm;
use gpui_kit::component::{
    list::ListItem,
    popover::{Popover, PopoverState},
    tree::{TreeItem, TreeState, tree},
};
#[cfg(test)]
mod tests;

pub(super) fn popover(owner: WeakEntity<Editor>, button: Button) -> Popover {
    Popover::new("provider-kind-popover")
        .p_1()
        .trigger(button)
        .content(move |_, window, cx| {
            let dismiss = cx.entity().downgrade();
            window.use_keyed_state("provider-kind-picker", cx, |window, cx| {
                Picker::new(owner.clone(), dismiss, window, cx)
            })
        })
}

struct Picker {
    owner: WeakEntity<Editor>,
    dismiss: WeakEntity<PopoverState>,
    tree: Entity<TreeState>,
    selected: Preset,
}

impl Picker {
    fn new(
        owner: WeakEntity<Editor>,
        dismiss: WeakEntity<PopoverState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected = owner.upgrade().unwrap().read(cx).preset.kind();
        let items = groups()
            .into_iter()
            .map(|(key, presets)| {
                TreeItem::new(format!("brand-{key}"), tr(key)).children(
                    presets
                        .into_iter()
                        .map(|preset| TreeItem::new(preset.key(), tr(preset.key()))),
                )
            })
            .collect::<Vec<_>>();
        let tree = cx.new(|cx| TreeState::new(cx).items(items));
        tree.update(cx, |state, cx| {
            state.set_selected_item(Some(&TreeItem::new(selected.key(), "")), cx);
            state.reveal_item(&selected.key().into(), ScrollStrategy::Center, cx);
            state.focus(window, cx);
        });
        cx.observe(&tree, |_, _, cx| cx.notify()).detach();
        Self {
            owner,
            dismiss,
            tree,
            selected,
        }
    }

    fn choose(&mut self, choice: Preset, window: &mut Window, cx: &mut Context<Self>) {
        _ = self.owner.update(cx, |editor, cx| {
            if !editor.closed
                && !editor.pending
                && editor.editing.is_none()
                && editor.preset.kind() != choice
            {
                editor.select_preset(choice, window, cx);
            }
        });
        _ = self
            .dismiss
            .update(cx, |state, cx| state.dismiss(window, cx));
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let choice = self
            .tree
            .read(cx)
            .selected_entry()
            .filter(|entry| !entry.is_folder())
            .and_then(|entry| choice(&entry.item().id));
        if let Some(choice) = choice {
            self.choose(choice, window, cx);
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }
}

fn choice(key: &str) -> Option<Preset> {
    Preset::all().find(|preset| preset.kind() == *preset && preset.key() == key)
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picker = cx.entity().downgrade();
        let selected = self.selected;
        let rows = (0..10)
            .take_while(|&index| self.tree.read(cx).entry(index).is_some())
            .count();
        div()
            .debug_selector(|| "provider-kind-picker".into())
            .w(px(320.).min(window.viewport_size().width - px(64.)))
            .h(px((rows * 32) as f32).min(window.viewport_size().height * 0.5))
            .capture_action(cx.listener(Self::confirm))
            .child(tree(&self.tree, move |index, entry, focused, _, _| {
                let key = entry.item().id.clone();
                let folder = entry.is_folder();
                let preset = (!folder).then(|| choice(&key)).flatten();
                let picker = picker.clone();
                let content = h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(div().size_4().when(folder, |slot| {
                        slot.child(
                            Icon::new(if entry.is_expanded() {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size_4(),
                        )
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .when(folder, |label| label.font_medium())
                            .child(entry.item().label.clone()),
                    )
                    .child(div().size_4().when(preset == Some(selected), |slot| {
                        slot.child(Icon::new(IconName::Check).size_4())
                    }));
                ListItem::new(index)
                    .debug_selector(move || format!("provider-kind-{key}"))
                    .selected(focused)
                    .h_8()
                    .px_2()
                    .pl(px(8. + entry.depth() as f32 * 16.))
                    .child(content)
                    .on_click(move |_, window, cx| {
                        if let Some(preset) = preset {
                            _ = picker.update(cx, |picker, cx| picker.choose(preset, window, cx));
                        }
                    })
            }))
    }
}
