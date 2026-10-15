//! Message navigation follows the behavior of Platform 727ce0a's
//! harbor_ai_message_navigation.dart. Kit has no waveform navigator; this small
//! drawing adapter uses Kit's message scroller and tooltip instead of a second list.
use super::*;
use gpui_kit::{
    base::{Placement, Positioner},
    component::{ActiveTheme, tooltip::Tooltip},
    prelude::FluentBuilder as _,
};
use sailry_protocol::conversation::Part;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const WIDTH: f32 = 50.;
const MIN_WIDTH: f32 = super::super::CONTENT_WIDTH + 2. * WIDTH;
const SLOT: f32 = 10.;
const HIT_PADDING: f32 = 4.;
const TICK_INSET: f32 = 2.;

pub(super) struct Navigation {
    pub(super) bounds: Rc<Cell<Bounds<Pixels>>>,
    width: Cell<Pixels>,
    split: bool,
    pub rows: Rc<RefCell<BTreeMap<TurnId, Bounds<Pixels>>>>,
    active: Rc<Cell<usize>>,
    hover: Option<usize>,
    dragging: bool,
    focus: FocusHandle,
}
impl Navigation {
    pub fn new(cx: &mut App) -> Self {
        Self {
            bounds: Default::default(),
            width: Default::default(),
            split: false,
            rows: Default::default(),
            active: Default::default(),
            hover: None,
            dragging: false,
            focus: cx.focus_handle(),
        }
    }
    pub(super) fn visible(&self) -> bool {
        !self.split && self.width.get() >= px(MIN_WIDTH)
    }
    pub(super) fn resize(&self, width: Pixels) -> bool {
        (self.width.replace(width) >= px(MIN_WIDTH)) != (width >= px(MIN_WIDTH))
    }
    pub(super) fn near_start(&self, turn: TurnId) -> bool {
        let area = self.bounds.get();
        self.rows.borrow().get(&turn).is_some_and(|row| {
            area.size.height > px(0.)
                && row.top() >= area.top() - px(80.)
                && row.top() < area.bottom()
        })
    }

    fn hit(&self, position: Point<Pixels>, count: usize) -> Option<usize> {
        let area = self.bounds.get();
        if count == 0 || !area.contains(&position) {
            return None;
        }
        let index = self.index(position, count);
        let height = f32::from(area.size.height);
        let x = f32::from(position.x - area.left()) - TICK_INSET;
        let y = f32::from(position.y - area.top()) - tick_y(height, count, index);
        let width = 5. + 25. * tick_strength(index, self.hover);
        // A small hit margin keeps thin marks usable without activating the empty rail.
        (x >= -HIT_PADDING
            && x <= width + HIT_PADDING
            && y.abs() <= (height / count as f32).min(SLOT) / 2.)
            .then_some(index)
    }

    fn index(&self, position: Point<Pixels>, count: usize) -> usize {
        index_at(
            f32::from(position.y - self.bounds.get().top()),
            f32::from(self.bounds.get().size.height),
            count,
        )
    }
}
fn index_at(y: f32, height: f32, count: usize) -> usize {
    if count == 0 || height <= 0. {
        return 0;
    }
    let extent = height.min(count as f32 * SLOT);
    (((y - (height - extent) / 2.) / extent * count as f32)
        .floor()
        .max(0.) as usize)
        .min(count - 1)
}

fn tick_strength(index: usize, hover: Option<usize>) -> f32 {
    hover.map_or(0., |hover| {
        let distance = index.abs_diff(hover) as f32;
        (-distance * distance / 12.).exp()
    })
}

fn tick_y(height: f32, count: usize, index: usize) -> f32 {
    let extent = height.min(count as f32 * SLOT);
    (height - extent) / 2. + extent * (index as f32 + 0.5) / count.max(1) as f32
}

impl View {
    pub(crate) fn set_split(&mut self, split: bool, cx: &mut Context<Self>) {
        if self.navigation.split != split {
            self.navigation.split = split;
            self.navigation.hover = None;
            self.navigation.dragging = false;
            cx.notify();
        }
    }

    fn navigate(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.rows.len() {
            return;
        }
        self.navigation.active.set(index);
        self.scroller.update(cx, |scroller, cx| {
            scroller.scroll_to_item(index, cx);
        });
        cx.notify();
    }

    pub(super) fn navigation(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if cx.has_active_drag()
            || !self
                .navigation
                .bounds
                .get()
                .contains(&window.mouse_position())
        {
            self.navigation.hover = None;
            self.navigation.dragging = false;
        }
        let count = self.rows.len();
        if !self.navigation.dragging
            && self
                .navigation
                .hit(window.mouse_position(), count)
                .is_none()
        {
            self.navigation.hover = None;
        }
        let bounds = self.navigation.bounds.clone();
        let rows = self.navigation.rows.clone();
        let turns = self.rows.clone();
        let active = self.navigation.active.clone();
        let hovered = self.navigation.hover;
        let tail = self.scroller.read(cx).is_following_tail();
        let foreground = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let control = div()
            .id("message-navigation")
            .flex()
            .debug_selector(|| "message-navigation".into())
            .w(px(WIDTH))
            .h_full()
            .pl_3()
            .role(Role::Slider)
            .aria_label(tr("chat_navigation"))
            .aria_value(format!("{}/{}", self.navigation.active.get() + 1, count))
            .tab_index(0)
            .track_focus(&self.navigation.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    let Some(index) = view.navigation.hit(event.position, view.rows.len()) else {
                        return;
                    };
                    view.navigation.hover = Some(index);
                    view.navigation.dragging = true;
                    view.navigation.focus.focus(window, cx);
                    view.navigate(index, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                if cx.has_active_drag()
                    || event.pressed_button.is_some() && !view.navigation.dragging
                {
                    if view.navigation.hover.take().is_some() {
                        cx.notify();
                    }
                    return;
                }
                let dragging =
                    view.navigation.dragging && event.pressed_button == Some(MouseButton::Left);
                let hovered = if dragging {
                    Some(view.navigation.index(event.position, view.rows.len()))
                } else {
                    view.navigation.hit(event.position, view.rows.len())
                };
                if view.navigation.hover != hovered {
                    view.navigation.hover = hovered;
                    cx.notify();
                }
                if dragging && let Some(index) = hovered {
                    view.navigate(index, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, _, cx| {
                    view.navigation.dragging = false;
                    let hovered = view.navigation.hit(event.position, view.rows.len());
                    if view.navigation.hover != hovered {
                        view.navigation.hover = hovered;
                        cx.notify();
                    }
                }),
            )
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if !hovered {
                    view.navigation.hover = None;
                    view.navigation.dragging = false;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                let active = view.navigation.active.get();
                let index = match event.keystroke.key.as_str() {
                    "up" => active.saturating_sub(1),
                    "down" => (active + 1).min(view.rows.len().saturating_sub(1)),
                    "home" => 0,
                    "end" => view.rows.len().saturating_sub(1),
                    _ => return,
                };
                view.navigate(index, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |area, _, _| {
                        bounds.set(area);
                        let current = if tail {
                            count.saturating_sub(1)
                        } else {
                            turns
                                .iter()
                                .enumerate()
                                .find_map(|(index, turn)| {
                                    rows.borrow()
                                        .get(turn)
                                        .filter(|row| {
                                            row.bottom() > area.top() + px(1.)
                                                && row.top() < area.bottom()
                                        })
                                        .map(|_| index)
                                })
                                .unwrap_or(active.get().min(count.saturating_sub(1)))
                        };
                        active.set(current);
                        current
                    },
                    move |area, current, window, _| {
                        if count == 0 {
                            return;
                        }
                        for index in 0..count {
                            let strength = tick_strength(index, hovered);
                            let width = 5. + 25. * strength;
                            let color = if index == current {
                                foreground
                            } else {
                                muted.opacity(0.32 + strength * 0.46)
                            };
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        area.left() + px(TICK_INSET),
                                        area.top()
                                            + px(tick_y(f32::from(area.size.height), count, index)),
                                    ),
                                    size(px(width), px(2.5)),
                                ),
                                color,
                            ));
                        }
                    },
                )
                .size_full(),
            )
            .when_some(hovered, |control, index| {
                let area = self.navigation.bounds.get();
                let anchor = Bounds::new(
                    point(
                        area.left(),
                        area.top() + px(tick_y(f32::from(area.size.height), count, index)),
                    ),
                    size(area.size.width, px(0.)),
                );
                let title = self.navigation_title(index);
                let preview = cx.new(|_| {
                    Tooltip::element(move |_, _| {
                        div()
                            .w(px(280.))
                            .whitespace_normal()
                            .debug_selector(|| "message-navigation-preview".into())
                            .child(title.clone())
                    })
                    .m_0()
                });
                // The whole-rail tooltip keeps its original mouse anchor. Position
                // Kit's tooltip at the current tick instead, using its shared positioner.
                control.child(
                    deferred(
                        Positioner::side(anchor)
                            .placement(Placement::Right)
                            .offset(px(8.))
                            .child(preview),
                    )
                    .with_priority(200),
                )
            });
        control.into_any_element()
    }

    fn navigation_title(&self, index: usize) -> String {
        let turn = self.rows.get(index);
        self.history
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.page.entries)
            .filter(|entry| Some(&entry.turn) == turn && entry.author == "user")
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::Text(text) => Some(text),
                _ => None,
            })
            .flat_map(|text| text.lines())
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| tr("chat_new").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::index_at;
    #[test]
    fn maps_track_geometry() {
        assert_eq!(index_at(0., 500., 3), 0);
        assert_eq!(index_at(250., 500., 3), 1);
        assert_eq!(index_at(500., 500., 3), 2);
        assert_eq!(index_at(50., 100., 100), 50);
        assert_eq!(index_at(100., 100., 100), 99);
        assert_eq!(index_at(0., 0., 0), 0);
    }
}
