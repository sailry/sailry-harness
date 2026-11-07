use super::*;
use libghostty_vt::mouse;
use sailry_protocol::terminal::{MouseAction, MouseButton, MouseEvent};

impl Vt {
    pub(super) fn encode_mouse(&mut self, event: &MouseEvent) -> Result<Vec<u8>, Fault> {
        let pixels = check(
            self.terminal.mode(Mode::SGR_PIXELS_MOUSE),
            "read Ghostty pixel mouse mode",
        )?;
        // Cell coordinates avoid cumulative rounding from fractional UI font metrics.
        // Pixel reporting uses the separate position supplied in viewport coordinates.
        let (width, height, x, y) = if pixels {
            (
                self.viewport.pixel_width.max(1),
                self.viewport.pixel_height.max(1),
                event.x,
                event.y,
            )
        } else {
            (
                u32::from(self.viewport.columns),
                u32::from(self.viewport.rows),
                u32::from(event.column),
                u32::from(event.row),
            )
        };
        if self.mouse_dirty {
            // Reconfiguration clears Ghostty's last-cell cache; keep it across input events.
            self.mouse_encoder
                .set_options_from_terminal(&self.terminal)
                .set_size(mouse::EncoderSize {
                    screen_width: width,
                    screen_height: height,
                    cell_width: 1,
                    cell_height: 1,
                    padding_top: 0,
                    padding_bottom: 0,
                    padding_left: 0,
                    padding_right: 0,
                })
                .set_track_last_cell(!pixels);
            self.mouse_dirty = false;
        }
        self.mouse_encoder
            .set_any_button_pressed(event.button.is_some() && event.action != MouseAction::Release);
        self.mouse_event
            .set_action(match event.action {
                MouseAction::Press => mouse::Action::Press,
                MouseAction::Release => mouse::Action::Release,
                MouseAction::Motion => mouse::Action::Motion,
            })
            .set_button(event.button.map(|button| match button {
                MouseButton::Left => mouse::Button::Left,
                MouseButton::Middle => mouse::Button::Middle,
                MouseButton::Right => mouse::Button::Right,
                MouseButton::WheelUp => mouse::Button::Four,
                MouseButton::WheelDown => mouse::Button::Five,
                MouseButton::WheelLeft => mouse::Button::Six,
                MouseButton::WheelRight => mouse::Button::Seven,
            }))
            .set_mods(input::ghostty_modifiers(event.modifiers))
            .set_position(mouse::Position {
                x: x.min(width - 1) as f32,
                y: y.min(height - 1) as f32,
            });
        let mut bytes = Vec::with_capacity(64);
        check(
            self.mouse_encoder
                .encode_to_vec(&self.mouse_event, &mut bytes),
            "encode Ghostty mouse event",
        )?;
        Ok(bytes)
    }
}
