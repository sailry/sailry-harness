//! Lists inside dialogs and popovers share one look: an optional heading, then one
//! bordered container whose rows are Kit ghost buttons separated by dividers. The
//! provider connection-method list is the visual template. Callers supply content and
//! handlers; Kit owns hover, focus and button sizing.
use gpui_kit::component::{
    button::{Button, ButtonRounded, ButtonVariants as _},
    checkbox::Checkbox,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::rc::Rc;

const SINGLE: f32 = 53.;
const DOUBLE: f32 = 64.;
/// Rows of a borderless list inside a popover.
const COMPACT: f32 = 44.;
/// Expanded content lines up with the title after the disclosure chevron.
const INDENT: f32 = 40.;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Frame = Box<dyn FnOnce(Stateful<Div>) -> Stateful<Div>>;

enum Mark {
    /// The row navigates elsewhere.
    Chevron,
    /// Single selection; only the chosen row shows the check.
    Selected,
    Remove(SharedString, Handler),
}

#[derive(IntoElement)]
pub(crate) struct List {
    id: SharedString,
    heading: Option<SharedString>,
    count: Option<SharedString>,
    rows: Vec<Row>,
    action: Option<AnyElement>,
    empty: Option<SharedString>,
    borderless: bool,
}

impl List {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            heading: None,
            count: None,
            rows: Vec::new(),
            action: None,
            empty: None,
            borderless: false,
        }
    }

    pub fn heading(mut self, label: impl Into<SharedString>) -> Self {
        self.heading = Some(label.into());
        self
    }

    /// Secondary text at the end of the heading, such as a count.
    pub fn count(mut self, text: impl Into<SharedString>) -> Self {
        self.count = Some(text.into());
        self
    }

    pub fn rows(mut self, rows: impl IntoIterator<Item = Row>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// The last row of the container, usually [`action`] or a popover around it.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    /// Shown in a dashed frame when there are no rows.
    pub fn empty(mut self, text: impl Into<SharedString>) -> Self {
        self.empty = Some(text.into());
        self
    }

    /// Popover content: no frame or dividers, compact rounded rows.
    pub fn borderless(mut self) -> Self {
        self.borderless = true;
        self
    }
}

/// A centered "+ label" row for [`List::action`].
pub(crate) fn action(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .ghost()
        .rounded(ButtonRounded::None)
        .w_full()
        .h(px(SINGLE))
        .px_4()
        .icon(IconName::Plus)
        .label(label)
}

impl RenderOnce for List {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let border = cx.theme().border;
        let heading = self.heading.map(|label| {
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(self.count)
        });
        let empty = self.rows.is_empty();
        let placeholder = self.empty.filter(|_| empty).map(|text| {
            div()
                .w_full()
                .py(px(22.))
                .px_4()
                .text_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .border_1()
                .border_dashed()
                .border_color(border)
                .rounded(cx.theme().radius_lg)
                .child(text)
        });
        let frame = |list: Div| {
            if self.borderless {
                list.gap_0p5()
            } else {
                list.border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .bg(cx.theme().secondary)
            }
        };
        let divided = !self.borderless;
        let rows = self.rows.into_iter().enumerate().map(|(index, mut row)| {
            row.divided = divided && index > 0;
            row.compact = self.borderless;
            row
        });
        let action = self.action.map(|action| {
            div()
                .w_full()
                .when(divided && !empty, |row| {
                    row.border_t_1().border_color(border)
                })
                .child(action)
        });
        let body = (!empty || action.is_some()).then(|| {
            frame(v_flex().w_full().min_w_0())
                .children(rows)
                .children(action)
        });
        v_flex()
            .id(self.id)
            .w_full()
            .min_w_0()
            .gap_2()
            .children(heading)
            .children(placeholder)
            .children(body)
    }
}

#[derive(IntoElement)]
pub(crate) struct Row {
    id: SharedString,
    title: SharedString,
    subtitle: Option<SharedString>,
    label: Option<SharedString>,
    leading: Option<AnyElement>,
    checked: Option<bool>,
    meta: Option<SharedString>,
    trailing: Vec<AnyElement>,
    mark: Option<Mark>,
    on_click: Option<Handler>,
    expanded: Option<bool>,
    content: Option<AnyElement>,
    frame: Option<Frame>,
    disabled: bool,
    divided: bool,
    compact: bool,
}

impl Row {
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            label: None,
            leading: None,
            checked: None,
            meta: None,
            trailing: Vec::new(),
            mark: None,
            on_click: None,
            expanded: None,
            content: None,
            frame: None,
            disabled: false,
            divided: false,
            compact: false,
        }
    }

    /// A second line; two-line rows are taller.
    pub fn subtitle(mut self, text: impl Into<SharedString>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    /// Accessible name when the title alone is not enough.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An icon tile or avatar before the text.
    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    /// A leading checkbox for multiple selection; clicking the row toggles it.
    pub fn checkbox(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Muted summary before the trailing controls.
    pub fn meta(mut self, text: impl Into<SharedString>) -> Self {
        self.meta = Some(text.into());
        self
    }

    /// Extra trailing controls. Their clicks never reach the row.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    pub fn chevron(mut self) -> Self {
        self.mark = Some(Mark::Chevron);
        self
    }

    /// Single selection: the chosen row shows a check without changing its fill.
    pub fn selected(mut self, selected: bool) -> Self {
        self.mark = selected.then_some(Mark::Selected);
        self
    }

    /// A trailing remove button labeled `label` for tooltips and accessibility.
    pub fn remove(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.mark = Some(Mark::Remove(label.into(), Rc::new(handler)));
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Makes the row expandable; `content` renders under it while expanded.
    /// The caller toggles the state from [`Row::on_click`].
    pub fn expanded(mut self, expanded: bool, content: impl IntoElement) -> Self {
        self.expanded = Some(expanded);
        self.content = expanded.then(|| content.into_any_element());
        self
    }

    /// Content under the row without a disclosure chevron, for rows whose click
    /// does something else, such as selection. Trailing controls may toggle it.
    pub fn details(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// Reorders by dragging: shows a grip and drops `payload` onto other rows.
    pub fn draggable<T: Render + Clone>(
        mut self,
        payload: T,
        drop: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.frame = Some(Box::new(move |frame: Stateful<Div>| {
            frame
                .on_drag(payload, |payload, _, _, cx| cx.new(|_| payload.clone()))
                .drag_over::<T>(|style, _, _, cx| style.bg(cx.theme().accent))
                .on_drop(move |payload: &T, window, cx| drop(payload, window, cx))
        }));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let height = px(match (self.compact, self.subtitle.is_some()) {
            (true, _) => COMPACT,
            (false, true) => DOUBLE,
            (false, false) => SINGLE,
        });
        let draggable = self.frame.is_some();
        let label = self.label.unwrap_or_else(|| self.title.clone());
        let mark = self.mark.map(|mark| match mark {
            Mark::Chevron => Icon::new(IconName::ChevronRight)
                .size_4()
                .text_color(muted)
                .into_any_element(),
            Mark::Selected => div()
                .size_5()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(cx.theme().blue)
                .child(
                    Icon::new(IconName::Check)
                        .size_3()
                        .text_color(cx.theme().background),
                )
                .into_any_element(),
            Mark::Remove(label, handler) => {
                Button::new(SharedString::from(format!("{}-remove", self.id)))
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip(label.clone())
                    .accessibility_label(label)
                    .disabled(self.disabled)
                    .on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        handler(event, window, cx);
                    })
                    .into_any_element()
            }
        });
        let controls = (!self.trailing.is_empty()).then(|| {
            // Only independent controls consume clicks; decorative marks open the row.
            h_flex()
                .id(SharedString::from(format!("{}-trailing", self.id)))
                .flex_shrink_0()
                .items_center()
                .gap_1()
                .on_click(|_, _, cx| cx.stop_propagation())
                .children(self.trailing)
        });
        let trailing = (controls.is_some() || mark.is_some()).then(|| {
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap_1()
                .children(controls)
                .children(mark)
        });
        let line = h_flex()
            .size_full()
            .min_w_0()
            .items_center()
            .gap_3()
            .when(draggable, |line| {
                line.child(
                    Icon::from(gpui_kit::assets::IconName::GripVertical)
                        .size_3()
                        .flex_shrink_0()
                        .text_color(muted.opacity(0.6)),
                )
            })
            .when_some(self.expanded, |line, expanded| {
                line.child(
                    Icon::new(IconName::ChevronRight)
                        .size_3p5()
                        .flex_shrink_0()
                        .text_color(muted)
                        .when(expanded, |icon| icon.rotate(percentage(0.25))),
                )
            })
            .when_some(self.checked, |line, checked| {
                // The row handles the click; the box only reflects the state.
                line.child(
                    Checkbox::new(SharedString::from(format!("{}-check", self.id)))
                        .checked(checked)
                        .tab_stop(false),
                )
            })
            .children(
                self.leading
                    .map(|leading| div().flex_shrink_0().child(leading)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .items_start()
                    .gap_0p5()
                    .child(
                        div()
                            .max_w_full()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .child(self.title),
                    )
                    .children(self.subtitle.map(|subtitle| {
                        div()
                            .max_w_full()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(subtitle)
                    })),
            )
            .children(self.meta.map(|meta| {
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(muted)
                    .child(meta)
            }))
            .children(trailing);
        let header = match self.on_click {
            Some(handler) => Button::new(self.id.clone())
                .ghost()
                .when(!self.compact, |button| button.rounded(ButtonRounded::None))
                .w_full()
                .h(height)
                .map(|button| {
                    if self.compact {
                        button.px_2p5()
                    } else {
                        button.px_4()
                    }
                })
                .disabled(self.disabled)
                .accessibility_label(label)
                .child(line)
                .on_click(move |event, window, cx| handler(event, window, cx))
                .into_any_element(),
            None => div()
                .w_full()
                .h(height)
                .map(|row| {
                    if self.compact {
                        row.px_2p5()
                    } else {
                        row.px_4()
                    }
                })
                .child(line)
                .into_any_element(),
        };
        let frame = v_flex()
            .id(SharedString::from(format!("{}-row", self.id)))
            .w_full()
            .min_w_0()
            .when(self.divided, |row| {
                row.border_t_1().border_color(cx.theme().border)
            })
            .when(self.disabled, |row| row.opacity(0.45))
            .child(header)
            .children(self.content.map(|content| {
                v_flex()
                    .w_full()
                    .min_w_0()
                    .pt_1()
                    .pb_4()
                    .pr_4()
                    .pl(px(INDENT))
                    .gap_3()
                    .child(content)
            }));
        match self.frame {
            Some(decorate) if !self.disabled => decorate(frame),
            _ => frame,
        }
    }
}
