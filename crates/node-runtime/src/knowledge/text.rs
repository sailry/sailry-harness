//! Literal Unicode keywords retain the original text outside the derived index.

#[cfg(test)]
mod tests;

fn cjk(character: char) -> bool {
    character.is_alphabetic()
        && matches!(character as u32,
            0x1100..=0x11ff | 0x3040..=0x30ff | 0x3130..=0x318f | 0x31f0..=0x31ff
            | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xa960..=0xa97f
            | 0xac00..=0xd7ff | 0xf900..=0xfaff | 0xff66..=0xff9d
            | 0x1b000..=0x1b16f | 0x20000..=0x323af)
}

fn separated(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        if cjk(character) {
            result.push(' ');
            result.push(character);
            result.push(' ');
        } else {
            result.push(character);
        }
    }
    result
}

/// Keep a complete phrase stream, followed by adjacent CJK bigrams.
pub(crate) fn index(text: &str) -> String {
    if !text.chars().any(cjk) {
        return String::new();
    }
    let mut result = separated(text);
    let mut previous = None;
    for character in text.chars() {
        if cjk(character) {
            if let Some(before) = previous {
                result.push(' ');
                result.push(before);
                result.push(character);
            }
            previous = Some(character);
        } else {
            previous = None;
        }
    }
    result
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// Literal phrases retain Latin words and match CJK through their unigram stream.
pub(crate) fn query(text: &str) -> String {
    if !text.chars().any(cjk) {
        return quoted(text);
    }
    quoted(&separated(text))
}
