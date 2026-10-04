use crate::Apply;
use sailry_protocol::{
    terminal::{self, Screen, ScreenUpdate, Snapshot},
    *,
};

pub struct Projection {
    node: NodeId,
    terminal: TerminalId,
    generation: u64,
    snapshot: Option<Snapshot>,
    recovery: bool,
}

impl Projection {
    pub fn new(node: NodeId, terminal: TerminalId, generation: u64) -> Self {
        Self {
            node,
            terminal,
            generation,
            snapshot: None,
            recovery: true,
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    pub fn reconnect(&mut self, generation: u64) -> Result<(), Fault> {
        if generation <= self.generation {
            return Err(invalid("connection generation must increase"));
        }
        self.generation = generation;
        self.recovery = true;
        Ok(())
    }

    pub fn apply(&mut self, generation: u64, update: Update) -> Result<Apply, Fault> {
        if generation != self.generation {
            return Ok(Apply::Ignored);
        }
        match update {
            Update::TerminalSnapshot(snapshot) => {
                self.check_target(snapshot.node, snapshot.info.id)?;
                if self.snapshot.as_ref().is_some_and(|current| {
                    snapshot.info.revision < current.info.revision
                        || (snapshot.info.revision == current.info.revision
                            && snapshot.sequence < current.sequence)
                }) {
                    return Ok(Apply::Ignored);
                }
                validate(&snapshot.screen)?;
                self.snapshot = Some(snapshot);
                self.recovery = false;
                Ok(Apply::Applied)
            }
            Update::TerminalFrame(frame) => {
                self.check_target(frame.node, frame.info.id)?;
                let Some(current) = &self.snapshot else {
                    return Ok(Apply::Recover);
                };
                if frame.sequence <= current.sequence {
                    return Ok(Apply::Ignored);
                }
                if self.recovery || current.sequence.checked_add(1) != Some(frame.sequence) {
                    self.recovery = true;
                    return Ok(Apply::Recover);
                }
                let screen = match frame.screen {
                    ScreenUpdate::Replace { screen } => screen,
                    ScreenUpdate::Patch { patch } => {
                        let drop = patch.dropped_scrollback_rows as usize;
                        if drop > current.screen.scrollback.len() {
                            self.recovery = true;
                            return Ok(Apply::Recover);
                        }
                        let mut scrollback = current.screen.scrollback[drop..].to_vec();
                        scrollback.extend(patch.appended_scrollback);
                        Screen {
                            columns: patch.columns,
                            scrollback,
                            rows: patch.rows,
                            cursor: patch.cursor,
                            foreground: patch.foreground,
                            background: patch.background,
                            cursor_color: patch.cursor_color,
                            bracketed_paste: patch.bracketed_paste,
                            mouse_tracking: patch.mouse_tracking,
                            alternate: patch.alternate,
                            features: patch.features,
                            graphics: patch
                                .graphics
                                .unwrap_or_else(|| current.screen.graphics.clone()),
                        }
                    }
                };
                validate(&screen)?;
                self.snapshot = Some(Snapshot {
                    node: frame.node,
                    info: frame.info,
                    sequence: frame.sequence,
                    screen,
                });
                Ok(Apply::Applied)
            }
            Update::ResetRequired => {
                self.recovery = true;
                Ok(Apply::Recover)
            }
            _ => Err(invalid("unexpected terminal subscription update")),
        }
    }

    fn check_target(&self, node: NodeId, terminal: TerminalId) -> Result<(), Fault> {
        if node != self.node || terminal != self.terminal {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "terminal update belongs to another resource",
            ));
        }
        Ok(())
    }
}

fn validate(screen: &Screen) -> Result<(), Fault> {
    if !(1..=terminal::MAX_COLUMNS).contains(&screen.columns)
        || screen.rows.is_empty()
        || screen.rows.len() > terminal::MAX_ROWS as usize
        || screen.scrollback.len() > terminal::MAX_SCROLLBACK_ROWS
    {
        return Err(invalid("terminal screen dimensions are invalid"));
    }
    if screen.cursor.is_some_and(|cursor| {
        cursor.column >= screen.columns || cursor.row as usize >= screen.rows.len()
    }) {
        return Err(invalid("terminal cursor is outside the viewport"));
    }
    let mut images = std::collections::BTreeMap::new();
    let mut allocation = 0_u64;
    for image in &screen.graphics.images {
        allocation =
            allocation.saturating_add(u64::from(image.width) * u64::from(image.height) * 4);
        if image.width == 0
            || image.height == 0
            || allocation > terminal::MAX_GRAPHICS_BYTES as u64
            || image.png.len() > terminal::MAX_GRAPHICS_BYTES * 2
            || images.insert(image.id, image).is_some()
        {
            return Err(invalid("terminal image resource is invalid"));
        }
    }
    for placement in &screen.graphics.placements {
        let Some(image) = images.get(&placement.image) else {
            return Err(invalid("terminal placement image is missing"));
        };
        let [x, y, width, height] = placement.source;
        if x.checked_add(width).is_none_or(|end| end > image.width)
            || y.checked_add(height).is_none_or(|end| end > image.height)
            || placement.cell.contains(&0)
            || placement
                .tile
                .is_some_and(|[column, row, columns, rows]| column >= columns || row >= rows)
        {
            return Err(invalid("terminal placement geometry is invalid"));
        }
    }
    for line in screen.rows.iter().chain(&screen.scrollback) {
        let mut end = 0;
        for span in &line.spans {
            if span.column < end || span.columns == 0 {
                return Err(invalid("terminal spans overlap"));
            }
            end = span
                .column
                .checked_add(span.columns)
                .filter(|end| *end <= screen.columns)
                .ok_or_else(|| invalid("terminal span is outside the viewport"))?;
        }
    }
    Ok(())
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests;
