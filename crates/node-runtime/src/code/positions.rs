use lsp_types::{Position, PositionEncodingKind};
use sailry_protocol::Fault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    pub fn from(value: Option<&PositionEncodingKind>) -> Result<Self, Fault> {
        match value {
            None => Ok(Self::Utf16),
            Some(value) if value == &PositionEncodingKind::UTF8 => Ok(Self::Utf8),
            Some(value) if value == &PositionEncodingKind::UTF16 => Ok(Self::Utf16),
            _ => Err(Fault::invalid(
                "language server selected an unsupported position encoding",
            )),
        }
    }
    pub fn units(self, value: &str) -> usize {
        match self {
            Self::Utf8 => value.len(),
            Self::Utf16 => value.encode_utf16().count(),
        }
    }

    pub fn at(
        self,
        text: &str,
        position: sailry_protocol::code::Position,
    ) -> Result<Position, Fault> {
        if position.line == 0 || position.column == 0 {
            return Err(Fault::invalid("code positions must be one-based"));
        }
        let line = text
            .split('\n')
            .nth((position.line - 1) as usize)
            .ok_or_else(|| Fault::invalid("code line is outside the document"))?
            .trim_end_matches('\r');
        let column = position.column as usize - 1;
        let offset = if column == line.chars().count() {
            line.len()
        } else {
            line.char_indices()
                .nth(column)
                .map(|(offset, _)| offset)
                .ok_or_else(|| Fault::invalid("code column is outside the document"))?
        };
        Ok(Position::new(
            position.line - 1,
            self.units(&line[..offset]) as u32,
        ))
    }

    pub fn symbol(self, text: &str, line: u32, symbol: &str) -> Result<Position, Fault> {
        if line == 0
            || symbol.is_empty()
            || symbol.len() > 1024
            || symbol.chars().any(char::is_control)
        {
            return Err(Fault::invalid(
                "expected a one-based line and a literal symbol",
            ));
        }
        let value = text
            .split('\n')
            .nth((line - 1) as usize)
            .ok_or_else(|| Fault::invalid("code line is outside the document"))?
            .trim_end_matches('\r');
        let mut matches = value.match_indices(symbol);
        let (offset, _) = matches
            .next()
            .ok_or_else(|| Fault::invalid("code symbol is absent from the requested line"))?;
        if matches.next().is_some() {
            return Err(Fault::invalid(
                "code symbol is ambiguous on the requested line",
            ));
        }
        Ok(Position::new(line - 1, self.units(&value[..offset]) as u32))
    }

    pub fn display(
        self,
        text: &str,
        position: Position,
    ) -> Option<sailry_protocol::code::Position> {
        let line = text
            .split('\n')
            .nth(position.line as usize)?
            .trim_end_matches('\r');
        let mut units = 0;
        let mut columns = 0;
        for character in line.chars() {
            if units == position.character as usize {
                return Some(sailry_protocol::code::Position {
                    line: position.line.checked_add(1)?,
                    column: columns + 1,
                });
            }
            units += match self {
                Self::Utf8 => character.len_utf8(),
                Self::Utf16 => character.len_utf16(),
            };
            columns += 1;
            if units > position.character as usize {
                return None;
            }
        }
        (units == position.character as usize).then_some(sailry_protocol::code::Position {
            line: position.line.checked_add(1)?,
            column: columns + 1,
        })
    }
}

#[cfg(test)]
mod tests;
