use crate::Apply;
use sailry_protocol::{
    terminal::{Screen, ScreenUpdate, Snapshot},
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
            return Err(sailry_protocol::Fault::invalid(
                "connection generation must increase",
            ));
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
                if snapshot.screen.validate().is_err() {
                    self.recovery = true;
                    return Ok(Apply::Recover);
                }
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
                if screen.validate().is_err() {
                    self.recovery = true;
                    return Ok(Apply::Recover);
                }
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
            _ => Err(sailry_protocol::Fault::invalid(
                "unexpected terminal subscription update",
            )),
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

#[cfg(test)]
mod tests;
