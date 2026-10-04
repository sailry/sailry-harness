// Adapted from sailry-code 67ae9fa0, terminal-host/src/vt.rs.
// See third_party_licenses/sailry-code-terminal.md.
mod effects;
#[cfg(test)]
mod feature_tests;
mod graphics;
mod input;
#[cfg(test)]
mod keyboard_tests;
mod mouse;
mod projection;
#[cfg(test)]
mod tests;
use projection::*;

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use libghostty_vt::{
    RenderState, Terminal as Ghostty, TerminalOptions,
    error::Error as GhosttyError,
    key::{
        Action as GhosttyKeyAction, Encoder as GhosttyKeyEncoder, Event as GhosttyKeyEvent,
        Key as GhosttyKey, Mods as GhosttyKeyModifiers,
    },
    paste,
    render::{CellIterator, CursorVisualStyle, Dirty, RowIterator},
    screen::{Cell, CellContentTag, CellWide, GridRef, Screen as ActiveScreen, TrackedGridRef},
    style::{Palette, PaletteIndex, RgbColor, Style, StyleColor, Underline},
    terminal::{ColorScheme as GhosttyColorScheme, Mode, Point, PointCoordinate, PointSpace},
};
use sailry_protocol::terminal::{
    Action, Appearance, ColorScheme, Cursor, CursorStyle, FUNCTION_KEY_COUNT, Input, Key, KeyEvent,
    Line, Modifiers, Patch, Rgb, Screen, ScreenUpdate, Span, TextStyle, UnderlineStyle, Viewport,
};

use sailry_protocol::{ErrorCode, Fault};

fn fail(message: String) -> Fault {
    Fault::new(ErrorCode::Internal, message)
}

pub(crate) struct Write {
    pub responses: Vec<Vec<u8>>,
    pub screen: Option<ScreenUpdate>,
}

pub(crate) struct Vt {
    terminal: Ghostty<'static, 'static>,
    key_encoder: GhosttyKeyEncoder<'static>,
    key_event: GhosttyKeyEvent<'static>,
    mouse_encoder: libghostty_vt::mouse::Encoder<'static>,
    mouse_event: libghostty_vt::mouse::Event<'static>,
    mouse_dirty: bool,
    render_state: RenderState<'static>,
    row_iterator: RowIterator<'static>,
    cell_iterator: CellIterator<'static>,
    responses: Rc<RefCell<Vec<Vec<u8>>>>,
    effects: Rc<RefCell<effects::Effects>>,
    graphics: graphics::Cache,
    viewport: Viewport,
    active_screen: ActiveScreen,
    max_scrollback_rows: usize,
    history_rows: usize,
    history_anchor: Option<TrackedGridRef>,
    screen: Screen,
    synchronized_since: Option<Instant>,
}

impl Vt {
    pub fn new(
        viewport: &Viewport,
        appearance: &Appearance,
        max_scrollback_rows: usize,
    ) -> Result<Self, Fault> {
        let responses = Rc::new(RefCell::new(Vec::new()));
        let mut terminal = check(
            Ghostty::new(TerminalOptions {
                cols: viewport.columns,
                rows: viewport.rows,
                max_scrollback: ghostty_scrollback_bytes(viewport, max_scrollback_rows),
            }),
            "create Ghostty terminal",
        )?;
        configure_appearance(&mut terminal, appearance)?;
        check(
            terminal.set_default_cursor_blink(Some(true)),
            "configure Ghostty cursor blink",
        )?;

        let callback_responses = Rc::clone(&responses);
        check(
            terminal.on_pty_write(move |_terminal, bytes| {
                callback_responses.borrow_mut().push(bytes.to_vec());
            }),
            "configure Ghostty PTY callback",
        )?;
        let color_scheme = match appearance.color_scheme {
            ColorScheme::Light => GhosttyColorScheme::Light,
            ColorScheme::Dark => GhosttyColorScheme::Dark,
        };
        check(
            terminal.on_color_scheme(move |_terminal| Some(color_scheme)),
            "configure Ghostty color scheme callback",
        )?;
        graphics::configure(&mut terminal)?;
        let effects = effects::register(&mut terminal)?;
        resize(&mut terminal, viewport)?;

        let active_screen = check(terminal.active_screen(), "read Ghostty screen")?;
        let mut vt = Self {
            terminal,
            key_encoder: check(GhosttyKeyEncoder::new(), "create Ghostty key encoder")?,
            key_event: check(GhosttyKeyEvent::new(), "create Ghostty key event")?,
            mouse_encoder: check(
                libghostty_vt::mouse::Encoder::new(),
                "create Ghostty mouse encoder",
            )?,
            mouse_event: check(
                libghostty_vt::mouse::Event::new(),
                "create Ghostty mouse event",
            )?,
            mouse_dirty: true,
            render_state: check(RenderState::new(), "create Ghostty render state")?,
            row_iterator: check(RowIterator::new(), "create Ghostty row iterator")?,
            cell_iterator: check(CellIterator::new(), "create Ghostty cell iterator")?,
            responses,
            effects,
            graphics: Default::default(),
            viewport: viewport.clone(),
            active_screen,
            max_scrollback_rows,
            history_rows: 0,
            history_anchor: None,
            screen: empty_screen(viewport, appearance),
            synchronized_since: None,
        };
        vt.replace_projection()?;
        Ok(vt)
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<Write, Fault> {
        self.responses.borrow_mut().clear();
        self.terminal.vt_write(bytes);
        self.mouse_dirty = true;
        let screen = self.present()?;
        let responses = std::mem::take(&mut *self.responses.borrow_mut());
        Ok(Write { responses, screen })
    }

    fn present(&mut self) -> Result<Option<ScreenUpdate>, Fault> {
        // Ghostty parses DEC 2026; the embedding renderer must hold the last
        // complete frame, including its cursor, until the application releases it.
        if check(
            self.terminal.mode(Mode::SYNC_OUTPUT),
            "read Ghostty synchronized output mode",
        )? {
            self.synchronized_since.get_or_insert_with(Instant::now);
            return Ok(None);
        }
        self.synchronized_since = None;
        self.screen_update().map(Some)
    }

    pub fn flush(&mut self, now: Instant, force: bool) -> Result<Option<ScreenUpdate>, Fault> {
        // Match Ghostty's one-second recovery for applications that omit DECRST.
        if self
            .synchronized_since
            .is_some_and(|since| force || now.duration_since(since) >= Duration::from_secs(1))
        {
            check(
                self.terminal.set_mode(Mode::SYNC_OUTPUT, false),
                "release Ghostty synchronized output",
            )?;
            self.synchronized_since = None;
            return self.screen_update().map(Some);
        }
        Ok(None)
    }

    pub fn resize(&mut self, viewport: &Viewport) -> Result<Screen, Fault> {
        let previous = self.viewport.clone();
        resize(&mut self.terminal, viewport)?;
        self.synchronized_since = None;
        self.viewport = viewport.clone();
        self.mouse_dirty = true;

        match self.replace_projection() {
            Ok(()) => Ok(self.screen.clone()),
            Err(error) => {
                if let Err(rollback) = resize(&mut self.terminal, &previous) {
                    return Err(fail(format!(
                        "{error}; Ghostty resize rollback failed: {rollback}"
                    )));
                }
                self.viewport = previous;
                if let Err(rollback) = self.replace_projection() {
                    return Err(fail(format!(
                        "{error}; Ghostty projection rollback failed: {rollback}"
                    )));
                }
                Err(error)
            }
        }
    }

    pub fn checkpoint(&self) -> Screen {
        self.screen.clone()
    }

    pub fn appearance(&mut self, appearance: &Appearance) -> Result<Write, Fault> {
        configure_appearance(&mut self.terminal, appearance)?;
        let scheme = match appearance.color_scheme {
            ColorScheme::Light => GhosttyColorScheme::Light,
            ColorScheme::Dark => GhosttyColorScheme::Dark,
        };
        check(
            self.terminal.on_color_scheme(move |_| Some(scheme)),
            "configure Ghostty color scheme callback",
        )?;
        let mut responses = Vec::new();
        if check(
            self.terminal.mode(Mode::COLOR_SCHEME_REPORT),
            "read Ghostty color scheme report mode",
        )? {
            let mut report = [0; 32];
            let length = check(
                scheme.encode_report(&mut report),
                "encode Ghostty color scheme report",
            )?;
            responses.push(report[..length].to_vec());
        }
        Ok(Write {
            responses,
            screen: self.present()?,
        })
    }

    fn screen_update(&mut self) -> Result<ScreenUpdate, Fault> {
        let active_screen = check(self.terminal.active_screen(), "read Ghostty screen")?;
        if active_screen != self.active_screen {
            return self.replace_screen_update();
        }

        match self.incremental_screen_update(active_screen) {
            Ok(Some(update)) => Ok(update),
            Ok(None) => self.replace_screen_update(),
            Err(incremental_error) => self.replace_screen_update().map_err(|rebuild_error| {
                fail(format!(
                    "Ghostty incremental projection failed: {incremental_error}; canonical reprojection failed: {rebuild_error}"
                ))
            }),
        }
    }

    fn incremental_screen_update(
        &mut self,
        active_screen: ActiveScreen,
    ) -> Result<Option<ScreenUpdate>, Fault> {
        let expected_cached_rows = self.history_rows.min(self.max_scrollback_rows);
        if self.screen.scrollback.len() != expected_cached_rows {
            return Err(fail(format!(
                "cached Ghostty scrollback {} does not match projected history window {expected_cached_rows}",
                self.screen.scrollback.len()
            )));
        }
        let current_history_rows = history_rows(&self.terminal)?;
        let Some((dropped_scrollback_rows, append_start)) =
            self.history_delta(current_history_rows)?
        else {
            return Ok(None);
        };

        let viewport = project_viewport(
            &self.terminal,
            &mut self.render_state,
            &mut self.row_iterator,
            &mut self.cell_iterator,
            &self.viewport,
        )?;
        let appended_scrollback = project_history(
            &self.terminal,
            append_start,
            current_history_rows,
            self.viewport.columns,
            &viewport.colors,
        )?;
        let history_anchor = track_history_tail(&self.terminal, current_history_rows)?;

        let dropped = usize::try_from(dropped_scrollback_rows)
            .map_err(|_| fail("Ghostty scrollback drop does not fit this platform".to_owned()))?;
        if dropped > self.screen.scrollback.len() {
            return Err(fail(format!(
                "Ghostty scrollback drop {dropped} exceeds cached history {}",
                self.screen.scrollback.len()
            )));
        }
        let mut scrollback = self.screen.scrollback[dropped..].to_vec();
        scrollback.extend(appended_scrollback.iter().cloned());
        if scrollback.len() > self.max_scrollback_rows {
            return Err(fail(format!(
                "projected Ghostty scrollback {} exceeds configured row limit {}",
                scrollback.len(),
                self.max_scrollback_rows
            )));
        }
        self.screen.scrollback = scrollback;
        self.screen.columns = self.viewport.columns;
        self.screen.rows.clone_from(&viewport.rows);
        self.screen.cursor = viewport.cursor;
        self.screen.foreground = viewport.foreground;
        self.screen.background = viewport.background;
        self.screen.cursor_color = viewport.cursor_color;
        self.screen.bracketed_paste = viewport.bracketed_paste;
        self.screen.mouse_tracking = viewport.mouse_tracking;
        self.screen.alternate = active_screen == ActiveScreen::Alternate;
        self.screen.features = self.features()?;
        let graphics = self.graphics.project(&self.terminal, &self.viewport)?;
        let changed_graphics = (graphics != self.screen.graphics).then(|| graphics.clone());
        self.screen.graphics = graphics;
        self.active_screen = active_screen;
        self.history_rows = current_history_rows;
        self.history_anchor = history_anchor;

        Ok(Some(ScreenUpdate::Patch {
            patch: Patch {
                dropped_scrollback_rows,
                appended_scrollback,
                columns: self.viewport.columns,
                rows: viewport.rows,
                cursor: viewport.cursor,
                foreground: viewport.foreground,
                background: viewport.background,
                cursor_color: viewport.cursor_color,
                bracketed_paste: viewport.bracketed_paste,
                mouse_tracking: viewport.mouse_tracking,
                alternate: active_screen == ActiveScreen::Alternate,
                features: self.screen.features.clone(),
                graphics: changed_graphics,
            },
        }))
    }

    fn replace_screen_update(&mut self) -> Result<ScreenUpdate, Fault> {
        self.replace_projection()?;
        Ok(ScreenUpdate::Replace {
            screen: self.screen.clone(),
        })
    }

    fn history_delta(&self, current_history_rows: usize) -> Result<Option<(u32, usize)>, Fault> {
        if self.history_rows == 0 {
            return Ok(Some((
                0,
                current_history_rows.saturating_sub(self.max_scrollback_rows),
            )));
        }
        let Some(anchor) = self.history_anchor.as_ref() else {
            return Ok(None);
        };
        let Some(point) = check(
            anchor.point(PointSpace::History),
            "resolve Ghostty scrollback anchor",
        )?
        else {
            return Ok(None);
        };
        let anchor_row = usize::try_from(point.y)
            .map_err(|_| fail("Ghostty scrollback anchor does not fit this platform".to_owned()))?;
        if anchor_row >= self.history_rows || anchor_row >= current_history_rows {
            return Ok(None);
        }
        let underlying_dropped = self.history_rows - anchor_row - 1;
        let cached_rows = self.screen.scrollback.len();
        let uncached_prefix = self.history_rows.saturating_sub(cached_rows);
        let dropped_from_cache = underlying_dropped
            .saturating_sub(uncached_prefix)
            .min(cached_rows);
        let retained_rows = cached_rows - dropped_from_cache;
        let first_appended_row = anchor_row + 1;
        let appended_rows = current_history_rows - first_appended_row;
        let overflow = retained_rows
            .saturating_add(appended_rows)
            .saturating_sub(self.max_scrollback_rows);
        let dropped_for_limit = overflow.min(retained_rows);
        let skipped_appended = overflow - dropped_for_limit;
        let dropped = dropped_from_cache + dropped_for_limit;
        let dropped = u32::try_from(dropped).map_err(|_| {
            fail("Ghostty scrollback drop does not fit the terminal protocol".to_owned())
        })?;
        Ok(Some((dropped, first_appended_row + skipped_appended)))
    }

    fn replace_projection(&mut self) -> Result<(), Fault> {
        let active_screen = check(self.terminal.active_screen(), "read Ghostty screen")?;
        let history_rows = history_rows(&self.terminal)?;
        let viewport = project_viewport(
            &self.terminal,
            &mut self.render_state,
            &mut self.row_iterator,
            &mut self.cell_iterator,
            &self.viewport,
        )?;
        let scrollback_start = history_rows.saturating_sub(self.max_scrollback_rows);
        let scrollback = project_history(
            &self.terminal,
            scrollback_start,
            history_rows,
            self.viewport.columns,
            &viewport.colors,
        )?;
        let history_anchor = track_history_tail(&self.terminal, history_rows)?;
        self.screen = Screen {
            columns: self.viewport.columns,
            scrollback,
            rows: viewport.rows,
            cursor: viewport.cursor,
            foreground: viewport.foreground,
            background: viewport.background,
            cursor_color: viewport.cursor_color,
            bracketed_paste: viewport.bracketed_paste,
            mouse_tracking: viewport.mouse_tracking,
            alternate: active_screen == ActiveScreen::Alternate,
            features: self.features()?,
            graphics: self.graphics.project(&self.terminal, &self.viewport)?,
        };
        self.active_screen = active_screen;
        self.history_rows = history_rows;
        self.history_anchor = history_anchor;
        Ok(())
    }
}

fn check<T>(result: Result<T, GhosttyError>, action: &str) -> Result<T, Fault> {
    result.map_err(|error| ghostty_error(action, error))
}

fn ghostty_error(action: &str, error: GhosttyError) -> Fault {
    fail(format!("{action}: {error}"))
}
