use super::{Item, Key, Kind, Page};
use crate::{preview, shell::Shell, tr};
use gpui_kit::base::Popover;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    list::{List, ListDelegate, ListItem, ListState},
    tag::Tag,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(super) struct Items {
    pub rows: Vec<Item>,
    pub generation: u64,
    selected: Option<IndexPath>,
    shell: WeakEntity<Shell>,
    key: Key,
}

impl Items {
    pub fn new(shell: WeakEntity<Shell>, key: Key) -> Self {
        Self {
            rows: Vec::new(),
            generation: 0,
            selected: None,
            shell,
            key,
        }
    }
}

impl ListDelegate for Items {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.rows.len()
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.rows.get(index.row)?;
        Some(
            ListItem::new(index.row).h_12().child(
                h_flex()
                    .debug_selector(move || format!("reference-row-{}", index.row))
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(Icon::new(row.icon()).size_4().flex_shrink_0())
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .line_height(relative(1.25))
                                    .truncate()
                                    .child(row.label()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .line_height(relative(1.25))
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(row.description()),
                            ),
                    )
                    .child(
                        div()
                            .w_4()
                            .flex_shrink_0()
                            .when(self.selected == Some(index), |slot| {
                                slot.debug_selector(|| "reference-selected".into())
                                    .child(Icon::new(IconName::Check).size_4())
                            }),
                    )
                    .when(matches!(row, Item::Page(..)), |row| {
                        row.child(Icon::new(IconName::ChevronRight).size_3().flex_shrink_0())
                    }),
            ),
        )
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(item) = self
            .selected
            .and_then(|index| self.rows.get(index.row))
            .cloned()
        else {
            return;
        };
        let shell = self.shell.clone();
        let key = self.key;
        let generation = self.generation;
        // Returning the List entity first allows the owner to replace its directory rows.
        window.defer(cx, move |window, cx| {
            _ = shell.update(cx, |shell, cx| {
                shell.choose_reference(key, generation, item, window, cx)
            });
        });
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let shell = self.shell.clone();
        let key = self.key;
        window.defer(cx, move |window, cx| {
            _ = shell.update(cx, |shell, cx| {
                if let Some(thread) = shell.conversations.get_mut(&key) {
                    thread.references.dismiss();
                    thread.input.update(cx, |input, cx| input.focus(window, cx));
                    cx.notify();
                }
            });
        });
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .debug_selector(|| "reference-empty".into())
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(tr("reference_empty"))
    }
}

impl Shell {
    pub(in crate::conversation) fn composer_references(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = (self.host, self.session);
        let thread = &self.conversations[&key];
        let state = &thread.references;
        let list = state.list.clone();
        let title = state.path.last().unwrap_or(&Page::Root).label();
        let owner = cx.entity().downgrade();
        Popover::new(format!("references-{}-{}", key.0, key.1))
            .anchor(Anchor::BottomLeft)
            .open(state.open)
            .track_focus(&thread.input.focus_handle(cx))
            .trigger_with(|_, _, _| div().w_full().h_0().into_any_element())
            .on_open_change(move |open, window, cx| {
                let open = *open;
                let owner = owner.clone();
                window.defer(cx, move |_, cx| {
                    _ = owner.update(cx, |shell, cx| {
                        if !open && let Some(thread) = shell.conversations.get_mut(&key) {
                            thread.references.dismiss();
                            cx.notify();
                        }
                    });
                });
            })
            .content(move |_, window, cx| {
                let rows = list.read(cx).delegate().rows.len();
                let height = (rows.clamp(1, 5) as f32 * 48.)
                    .min((f32::from(window.viewport_size().height) - 280.).max(48.));
                v_flex()
                    .popover_style(cx)
                    .p_1()
                    .bottom_1()
                    .debug_selector(|| "reference-picker".into())
                    .w(px(340.).min(window.viewport_size().width - px(48.)))
                    .gap_1()
                    .child(
                        h_flex().gap_1().px_1().min_w_0().child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px_1()
                                .py_1()
                                .text_sm()
                                .truncate()
                                .child(title.clone()),
                        ),
                    )
                    .child(div().h(px(height)).child(List::new(&list).h_full()))
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("reference_preview")),
                    )
            })
    }

    pub(in crate::conversation) fn sent_references(
        &self,
        key: Key,
        turn: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let references = &self
            .conversations
            .get(&key)?
            .turns
            .get(turn)?
            .options
            .as_ref()?
            .references;
        if references.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .gap_1()
                .flex_wrap()
                .min_w_0()
                .children(references.iter().enumerate().map(|(index, reference)| {
                    let reference = reference.clone();
                    let file = match &reference.kind {
                        Kind::File(path) => Some(path.clone()),
                        _ => None,
                    };
                    let directory = matches!(reference.kind, Kind::Directory(_));
                    let link = file.is_some() || directory;
                    if !link {
                        return Tag::secondary()
                            .small()
                            .rounded_full()
                            .max_w_48()
                            .gap_1()
                            .child(Icon::new(reference.icon()).size_3())
                            .child(div().truncate().child(reference.label.clone()))
                            .into_any_element();
                    }
                    Button::new(("sent-reference", index))
                        .ghost()
                        .small()
                        .rounded_full()
                        .max_w_48()
                        .icon(reference.icon())
                        .label(reference.label.clone())
                        .tooltip(reference.label.clone())
                        .debug_selector(move || format!("sent-reference-{turn}-{index}"))
                        .on_click(cx.listener(move |shell, _, window, cx| {
                            if key != (shell.host, shell.session)
                                || shell.page != preview::Page::Conversation
                                || shell
                                    .workspace
                                    .sessions
                                    .get(&key)
                                    .is_none_or(|session| session.owner != reference.owner)
                            {
                                return;
                            }
                            if let Some(path) = &file {
                                shell.open_conversation_link(path.clone().into(), window, cx);
                            } else if directory {
                                shell.open_resource_panel(preview::Page::Files, window, cx);
                            }
                        }))
                        .into_any_element()
                }))
                .into_any_element(),
        )
    }
}
