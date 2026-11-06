//! Codex patch grammar adapted from openai/codex
//! 451787127c0e70247034c33e358536ffd2b89cf8 codex-rs/apply-patch/src/parser.rs and
//! streaming_parser.rs (Apache-2.0). Error messages and the heredoc tolerance follow
//! Codex. Paths stay worktree-relative strings for the confined file service; the
//! Codex `*** Environment ID:` preamble has no Sailry meaning and is not a hunk header.
use super::{Chunk, Hunk};
use sailry_protocol::Fault;

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File: ";
const DELETE: &str = "*** Delete File: ";
const UPDATE: &str = "*** Update File: ";
const MOVE_TO: &str = "*** Move to: ";
const END_OF_FILE: &str = "*** End of File";
const CONTEXT: &str = "@@ ";
const EMPTY_CONTEXT: &str = "@@";

/// Parses a complete patch. A patch wrapped in an `<<EOF` heredoc is accepted, as in Codex.
pub(crate) fn parse(input: &str) -> Result<Vec<Hunk>, Fault> {
    let lines: Vec<&str> = input.trim().lines().collect();
    let lines = boundaries(&lines)?;
    let mut parser = Parser::default();
    parser.push(&lines.join("\n"))?;
    parser.finish()
}

fn boundaries<'a>(lines: &'a [&'a str]) -> Result<&'a [&'a str], Fault> {
    let error = match strict(lines) {
        Ok(()) => return Ok(lines),
        Err(error) => error,
    };
    match lines {
        [first, .., last]
            if matches!(*first, "<<EOF" | "<<'EOF'" | "<<\"EOF\"")
                && last.ends_with("EOF")
                && lines.len() >= 4 =>
        {
            let inner = &lines[1..lines.len() - 1];
            strict(inner).map(|()| inner)
        }
        _ => Err(error),
    }
}

fn strict(lines: &[&str]) -> Result<(), Fault> {
    let first = lines.first().map(|line| line.trim());
    let last = lines.last().map(|line| line.trim());
    match (first, last) {
        (Some(BEGIN), Some(END)) => Ok(()),
        (Some(first), _) if first != BEGIN => Err(patch(
            "The first line of the patch must be '*** Begin Patch'",
        )),
        _ => Err(patch("The last line of the patch must be '*** End Patch'")),
    }
}

fn patch(message: &str) -> Fault {
    Fault::invalid(format!("invalid patch: {message}"))
}

fn hunk(line: usize, message: impl std::fmt::Display) -> Fault {
    Fault::invalid(format!("invalid hunk at line {line}, {message}"))
}

fn unknown(line: &str) -> String {
    format!(
        "'{line}' is not a valid hunk header. Valid hunk headers: '*** Add File: {{path}}', '*** Delete File: {{path}}', '*** Update File: {{path}}'"
    )
}

fn unexpected(line: &str) -> String {
    format!(
        "Unexpected line found in update hunk: '{line}'. Every line should start with ' ' (context line), '+' (added line), or '-' (removed line)"
    )
}

fn unmarked(line: &str) -> String {
    format!("Expected update hunk to start with a @@ context marker, got: '{line}'")
}

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Waiting,
    Started,
    Add,
    Delete,
    Update {
        header: usize,
    },
    Ended,
}

#[derive(Default)]
struct Parser {
    buffer: String,
    mode: Mode,
    hunks: Vec<Hunk>,
    line: usize,
}

impl Parser {
    fn push(&mut self, text: &str) -> Result<(), Fault> {
        for character in text.chars() {
            if character == '\n' {
                let mut line = std::mem::take(&mut self.buffer);
                line.truncate(line.strip_suffix('\r').map_or(line.len(), str::len));
                self.line += 1;
                self.process(&line)?;
            } else {
                self.buffer.push(character);
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<Hunk>, Fault> {
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.line += 1;
            if line.trim() == END {
                self.nonempty(line.trim())?;
                self.mode = Mode::Ended;
            } else {
                self.process(&line)?;
            }
        }
        if !matches!(self.mode, Mode::Ended) {
            return Err(patch("The last line of the patch must be '*** End Patch'"));
        }
        Ok(self.hunks)
    }

    /// An update needs at least one chunk unless it only moves the file, and its last
    /// chunk needs a line.
    fn nonempty(&self, line: &str) -> Result<(), Fault> {
        if let Some(Hunk::Update {
            path,
            destination,
            chunks,
        }) = self.hunks.last()
        {
            if chunks.is_empty()
                && destination.is_none()
                && let Mode::Update { header } = self.mode
            {
                return Err(hunk(
                    header,
                    format!("Update file hunk for path '{path}' is empty"),
                ));
            }
            if chunks.last().is_some_and(Chunk::is_empty) {
                if line == END {
                    return Err(hunk(self.line, "Update hunk does not contain any lines"));
                }
                return Err(hunk(self.line, unexpected(line)));
            }
        }
        Ok(())
    }

    fn header(&mut self, marker: &str) -> Result<bool, Fault> {
        if marker == END {
            self.nonempty(marker)?;
            self.mode = Mode::Ended;
            return Ok(true);
        }
        let (entry, mode) = if let Some(path) = marker.strip_prefix(ADD) {
            (
                Hunk::Add {
                    path: path.into(),
                    contents: String::new(),
                },
                Mode::Add,
            )
        } else if let Some(path) = marker.strip_prefix(DELETE) {
            (Hunk::Delete { path: path.into() }, Mode::Delete)
        } else if let Some(path) = marker.strip_prefix(UPDATE) {
            (
                Hunk::Update {
                    path: path.into(),
                    destination: None,
                    chunks: Vec::new(),
                },
                Mode::Update { header: self.line },
            )
        } else {
            return Ok(false);
        };
        self.nonempty(marker)?;
        self.hunks.push(entry);
        self.mode = mode;
        Ok(true)
    }

    fn process(&mut self, line: &str) -> Result<(), Fault> {
        let trimmed = line.trim();
        match self.mode {
            Mode::Waiting if trimmed == BEGIN => {
                self.mode = Mode::Started;
                Ok(())
            }
            Mode::Waiting => Err(patch(
                "The first line of the patch must be '*** Begin Patch'",
            )),
            Mode::Started | Mode::Delete => {
                if self.header(trimmed)? {
                    return Ok(());
                }
                Err(hunk(self.line, unknown(trimmed)))
            }
            Mode::Add => {
                if self.header(trimmed)? {
                    return Ok(());
                }
                if let Some(text) = line.strip_prefix('+')
                    && let Some(Hunk::Add { contents, .. }) = self.hunks.last_mut()
                {
                    contents.push_str(text);
                    contents.push('\n');
                    return Ok(());
                }
                Err(hunk(self.line, unknown(trimmed)))
            }
            Mode::Update { .. } => self.update(line),
            Mode::Ended if trimmed.is_empty() => Ok(()),
            Mode::Ended => Err(patch("The last line of the patch must be '*** End Patch'")),
        }
    }

    /// Indented markers stay context lines, so only trailing whitespace is ignored here.
    fn update(&mut self, line: &str) -> Result<(), Fault> {
        let marker = line.trim_end();
        if self.header(marker)? {
            return Ok(());
        }
        let number = self.line;
        let Some(Hunk::Update {
            destination,
            chunks,
            ..
        }) = self.hunks.last_mut()
        else {
            return Err(hunk(number, unexpected(line)));
        };
        if chunks.last().is_some_and(|chunk| chunk.end_of_file) {
            if marker.is_empty() {
                return Ok(());
            }
            if marker != EMPTY_CONTEXT && !marker.starts_with(CONTEXT) {
                return Err(hunk(number, unmarked(line)));
            }
        }
        if chunks.is_empty()
            && destination.is_none()
            && let Some(path) = marker.strip_prefix(MOVE_TO)
        {
            *destination = Some(path.into());
            return Ok(());
        }
        let context = marker == EMPTY_CONTEXT || marker.starts_with(CONTEXT);
        if context && chunks.last().is_some_and(Chunk::is_empty) {
            return Err(hunk(number, unexpected(line)));
        }
        if marker == EMPTY_CONTEXT {
            chunks.push(Chunk::default());
            return Ok(());
        }
        if let Some(text) = marker.strip_prefix(CONTEXT) {
            chunks.push(Chunk {
                context: Some(text.into()),
                ..Chunk::default()
            });
            return Ok(());
        }
        if marker == END_OF_FILE {
            if chunks.last().is_some_and(Chunk::is_empty) {
                return Err(hunk(number, "Update hunk does not contain any lines"));
            }
            if let Some(chunk) = chunks.last_mut() {
                chunk.end_of_file = true;
            }
            return Ok(());
        }
        if line.is_empty() {
            current(chunks).keep(String::new());
            return Ok(());
        }
        if let Some(text) = line.strip_prefix(' ') {
            current(chunks).keep(text.into());
            return Ok(());
        }
        if let Some(text) = line.strip_prefix('+') {
            current(chunks).new.push(text.into());
            return Ok(());
        }
        if let Some(text) = line.strip_prefix('-') {
            current(chunks).old.push(text.into());
            return Ok(());
        }
        if chunks.last().is_some_and(|chunk| !chunk.is_empty()) {
            return Err(hunk(number, unmarked(line)));
        }
        Err(hunk(number, unexpected(line)))
    }
}

/// The first diff line of an update may start its chunk without an `@@` marker.
fn current(chunks: &mut Vec<Chunk>) -> &mut Chunk {
    if chunks.is_empty() {
        chunks.push(Chunk::default());
    }
    chunks.last_mut().expect("a chunk was just ensured")
}
