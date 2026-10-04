use super::{View, protocol};
use gpui_kit::*;

#[derive(Default)]
pub(super) struct State {
    pressed: Option<protocol::MouseButton>,
    wheel: Point<Pixels>,
}

impl View {
    fn tracking(&self, modifiers: Modifiers) -> bool {
        !modifiers.shift
            && !self.selection.dragging
            && self.scroll.at_bottom()
            && self
                .state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.screen.mouse_tracking)
    }

    fn report(
        &mut self,
        action: protocol::MouseAction,
        button: Option<protocol::MouseButton>,
        position: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if !self.controlling() {
            return;
        }
        let snapshot = self.state.snapshot.as_ref().unwrap();
        let x = (position.x - self.metrics.bounds.left()).max(px(0.));
        let y = (position.y - self.metrics.bounds.top()).max(px(0.));
        let event = protocol::MouseEvent {
            action,
            button,
            column: (x / self.metrics.cell.width)
                .floor()
                .min((snapshot.screen.columns - 1) as f32) as u16,
            row: (y / self.metrics.cell.height)
                .floor()
                .min((snapshot.screen.rows.len() - 1) as f32) as u16,
            x: x.floor().into(),
            y: y.floor().into(),
            modifiers: protocol::Modifiers {
                shift: modifiers.shift,
                control: modifiers.control,
                alt: modifiers.alt,
                super_key: modifiers.platform,
                ..Default::default()
            },
        };
        self.send(
            sailry_protocol::Command::InputTerminal {
                terminal: self.binding.id,
                revision: snapshot.info.revision,
                input: protocol::Input::Mouse { event },
            },
            cx,
        );
    }

    pub(super) fn report_press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        if !self.tracking(event.modifiers) {
            return false;
        }
        let button = button(event.button);
        self.mouse.pressed = button;
        self.report(
            protocol::MouseAction::Press,
            button,
            event.position,
            event.modifiers,
            cx,
        );
        cx.stop_propagation();
        true
    }

    pub(super) fn report_release(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.mouse.pressed == button(event.button) && self.mouse.pressed.is_some() {
            self.mouse.pressed = None;
            self.report(
                protocol::MouseAction::Release,
                button(event.button),
                event.position,
                event.modifiers,
                cx,
            );
            cx.stop_propagation();
        }
    }

    pub(super) fn report_motion(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        if !self.tracking(event.modifiers) {
            return false;
        }
        self.report(
            protocol::MouseAction::Motion,
            self.mouse.pressed,
            event.position,
            event.modifiers,
            cx,
        );
        cx.stop_propagation();
        true
    }

    pub(super) fn report_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let alternate = self.state.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.screen.alternate && snapshot.screen.features.alternate_scroll
        });
        if event.modifiers.shift || (!self.tracking(event.modifiers) && !alternate) {
            return false;
        }
        let delta = event.delta.pixel_delta(self.metrics.cell.height);
        self.mouse.wheel += delta;
        let horizontal = delta.x.abs() > delta.y.abs();
        let remainder = if horizontal {
            &mut self.mouse.wheel.x
        } else {
            &mut self.mouse.wheel.y
        };
        let count = (*remainder / self.metrics.cell.height).trunc() as i32;
        *remainder -= self.metrics.cell.height * count as f32;
        for _ in 0..count.unsigned_abs().min(20) {
            if self.tracking(event.modifiers) {
                let button = match (horizontal, count > 0) {
                    (false, true) => protocol::MouseButton::WheelUp,
                    (false, false) => protocol::MouseButton::WheelDown,
                    (true, true) => protocol::MouseButton::WheelLeft,
                    (true, false) => protocol::MouseButton::WheelRight,
                };
                self.report(
                    protocol::MouseAction::Press,
                    Some(button),
                    event.position,
                    event.modifiers,
                    cx,
                );
            } else if self.controlling() && !horizontal {
                self.input(
                    protocol::Input::Key {
                        event: protocol::KeyEvent {
                            key: if count > 0 {
                                protocol::Key::ArrowUp
                            } else {
                                protocol::Key::ArrowDown
                            },
                            action: protocol::Action::Press,
                            modifiers: Default::default(),
                            utf8: None,
                            unshifted_codepoint: None,
                        },
                    },
                    cx,
                );
            }
        }
        cx.stop_propagation();
        true
    }

    pub(super) fn middle_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        if !self.report_press(event, cx) {
            self.paste(&super::input::Paste, window, cx);
        }
    }
}

fn button(button: MouseButton) -> Option<protocol::MouseButton> {
    match button {
        MouseButton::Left => Some(protocol::MouseButton::Left),
        MouseButton::Right => Some(protocol::MouseButton::Right),
        MouseButton::Middle => Some(protocol::MouseButton::Middle),
        _ => None,
    }
}
