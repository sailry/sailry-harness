use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    list::{List, ListDelegate, ListItem},
};
use gpui_kit::prelude::FluentBuilder as _;

pub(super) struct Items {
    pub rows: Vec<Item>,
    pub generation: u64,
    selected: Option<IndexPath>,
    owner: WeakEntity<View>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Context,
    Commands,
    Plugins,
    Skills,
}
impl Group {
    fn of(item: &Item) -> Self {
        match item {
            Item::Plugin(_) => Self::Plugins,
            Item::Command(commands::Choice::Skill(..)) => Self::Skills,
            Item::Command(_) | Item::Page(Page::Models | Page::Reasoning) => Self::Commands,
            _ => Self::Context,
        }
    }
    fn label(self) -> SharedString {
        tr(match self {
            Self::Context => "reference_group_context",
            Self::Commands => "composer_commands",
            Self::Plugins => "settings_plugins",
            Self::Skills => "settings_skills",
        })
    }
}

impl Items {
    pub fn new(owner: WeakEntity<View>) -> Self {
        Self {
            rows: vec![],
            generation: 0,
            selected: None,
            owner,
        }
    }
    fn sections(&self) -> Vec<std::ops::Range<usize>> {
        let mut sections: Vec<std::ops::Range<usize>> = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            if index == 0 || Group::of(row) != Group::of(&self.rows[index - 1]) {
                sections.push(index..index + 1);
            } else {
                sections.last_mut().unwrap().end += 1;
            }
        }
        sections
    }
    pub fn flat_index(&self, index: IndexPath) -> Option<usize> {
        let range = self.sections().get(index.section)?.clone();
        (index.row < range.len()).then_some(range.start + index.row)
    }
    pub fn index(&self, flat: usize) -> IndexPath {
        let (section, range) = self
            .sections()
            .into_iter()
            .enumerate()
            .find(|(_, range)| range.contains(&flat))
            .unwrap();
        IndexPath::default()
            .section(section)
            .row(flat - range.start)
    }
    pub fn item(&self, index: IndexPath) -> Option<&Item> {
        self.rows.get(self.flat_index(index)?)
    }
}

impl ListDelegate for Items {
    type Item = ListItem;

    fn sections_count(&self, _: &App) -> usize {
        self.sections().len().max(1)
    }
    fn items_count(&self, section: usize, _: &App) -> usize {
        self.sections().get(section).map_or(0, |range| range.len())
    }
    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let range = self.sections().get(section)?.clone();
        Some(
            div()
                .h_7()
                .px_2()
                .flex()
                .items_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Group::of(&self.rows[range.start]).label()),
        )
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let flat = self.flat_index(index)?;
        let row = self.rows.get(flat)?;
        Some(
            ListItem::new(flat).h_8().disabled(matches!(row, Item::Command(commands::Choice::External(entry)) if !entry.state.enabled)).child(
                h_flex()
                    .debug_selector(move || format!("live-reference-row-{flat}"))
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(row.emblem(cx))
                    .child(div().min_w_0().text_sm().truncate().child(row.label()))
                    .when(!row.description().is_empty(), |line| {
                        line.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(row.description()),
                        )
                    })
                    .when_some(
                        match row {
                            Item::Command(commands::Choice::Skill(package, _)) => {
                                Some(package.name.clone())
                            }
                            _ => None,
                        },
                        |line, name| {
                            line.child(
                                div()
                                    .ml_auto()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(name),
                            )
                        },
                    )
                    .when(matches!(row, Item::Page(_)), |row| {
                        row.child(Icon::new(IconName::ChevronRight).size_3().ml_auto())
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
        let Some(item) = self.selected.and_then(|index| self.item(index)).cloned() else {
            return;
        };
        let owner = self.owner.clone();
        let generation = self.generation;
        window.defer(cx, move |window, cx| {
            _ = owner.update(cx, |view, cx| {
                view.choose_reference(generation, item, window, cx)
            });
        });
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let owner = self.owner.clone();
        window.defer(cx, move |window, cx| {
            _ = owner.update(cx, |view, cx| {
                view.references.dismiss();
                view.input.update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
            });
        });
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(tr("reference_empty"))
    }
}

impl View {
    pub(in crate::conversation::live) fn sent_references(
        &self,
        turn: TurnId,
        references: &[Reference],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_1()
            .flex_wrap()
            .mb_2()
            .children(references.iter().enumerate().map(|(index, reference)| {
                let reference = reference.clone();
                Button::new(format!("sent-reference-{turn}-{index}"))
                    .ghost()
                    .small()
                    .max_w_64()
                    .label(inline::marker(&reference))
                    .debug_selector(move || format!("sent-reference-{turn}-{index}"))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.open_reference(reference.clone(), Some(turn), window, cx);
                    }))
            }))
            .into_any_element()
    }

    pub(in crate::conversation::live) fn reference_picker(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::base::{ElementExt as _, Popup};
        let state = &self.references;
        let bounds = state.bounds;
        // Kit Popup measures a zero-height anchor at the composer's top edge.
        // Keep its width tied to layout while the editor retains keyboard focus.
        let measure = cx.listener(|view, bounds: &Bounds<Pixels>, _, cx| {
            if view.references.bounds != *bounds {
                view.references.bounds = *bounds;
                cx.notify();
            }
        });
        let anchor = div()
            .relative()
            .w_full()
            .h_0()
            .on_prepaint(move |bounds, window, cx| measure(&bounds, window, cx));
        let popup = Popup::new("live-references", anchor)
            .w_full()
            .h_0()
            .anchor(Anchor::BottomLeft);
        if !state.open || bounds.size.width <= px(0.) {
            return popup.into_any_element();
        }
        let nested = state.page != self.reference_root();
        let commands = matches!(state.page, Page::Commands | Page::Root);
        let height = ((state.list.read(cx).delegate().rows.len().max(1) as f32 * 32.
            + state.list.read(cx).delegate().sections().len() as f32 * 28.)
            .min(320.))
        .min((f32::from(bounds.top()) - 100.).max(32.));
        let content = v_flex()
            .id("live-reference-picker")
            .debug_selector(|| "live-reference-picker".into())
            .popover_style(cx)
            .w(bounds.size.width)
            .bottom_1p5()
            .p_1()
            .gap_1()
            .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                view.references.dismiss();
                cx.notify();
            }))
            .when(nested, |body| {
                body.child(
                    h_flex().min_w_0().gap_1().child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .child(state.page.label()),
                    ),
                )
            })
            .when(!state.loading || commands, |body| {
                body.child(div().h(px(height)).child(List::new(&state.list).h_full()))
            })
            .when(state.loading, |body| {
                body.child(div().p_1().text_sm().child(tr(if commands {
                    "plugins_loading"
                } else {
                    "files_loading"
                })))
            })
            .when(state.error || state.next.is_some(), |body| {
                body.child(
                    Button::new("live-reference-more")
                        .rounded_full()
                        .ghost()
                        .small()
                        .label(tr(if state.error {
                            "chat_retry"
                        } else {
                            "files_load_more"
                        }))
                        .disabled(state.loading)
                        .debug_selector(|| "live-reference-more".into())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            if commands {
                                view.load_skills(window, cx);
                            } else {
                                view.load_references(view.references.next.is_some(), window, cx);
                            }
                        })),
                )
            });
        popup.content(content).into_any_element()
    }
}
