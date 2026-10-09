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

/// One explicit plugin term remains a literal phrase.
pub(crate) fn phrase(text: &str) -> String {
    if !text.chars().any(cjk) {
        return quoted(text);
    }
    quoted(&separated(text))
}

/// Natural-language queries match independent words and adjacent CJK bigrams.
pub(crate) fn query(text: &str) -> String {
    let mut terms = std::collections::BTreeSet::new();
    let mut word = String::new();
    let mut ideographs = Vec::new();
    for character in text.chars() {
        if cjk(character) {
            take_word(&mut terms, &mut word);
            ideographs.push(character);
        } else {
            take_bigrams(&mut terms, &mut ideographs);
            if character.is_alphanumeric() || matches!(character, '+' | '#') {
                word.push(character);
            } else {
                take_word(&mut terms, &mut word);
            }
        }
    }
    take_word(&mut terms, &mut word);
    take_bigrams(&mut terms, &mut ideographs);
    if terms.is_empty() {
        return quoted("");
    }
    terms
        .into_iter()
        .map(|term| quoted(&term))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn take_word(terms: &mut std::collections::BTreeSet<String>, word: &mut String) {
    if !word.is_empty() {
        terms.insert(std::mem::take(word));
    }
}

fn take_bigrams(terms: &mut std::collections::BTreeSet<String>, characters: &mut Vec<char>) {
    if characters.len() == 1 {
        terms.insert(characters[0].to_string());
    } else {
        terms.extend(characters.windows(2).map(|pair| pair.iter().collect()));
    }
    characters.clear();
}
