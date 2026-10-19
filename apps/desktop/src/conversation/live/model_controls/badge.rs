//! Short channel badges for the composer model chip; full names stay in tooltips and the picker.
use std::collections::{BTreeMap, BTreeSet};

/// Badge text for `name`, extended only as far as needed to differ from the
/// badges of the other configured channel `names`.
pub(super) fn resolve<'a>(name: &str, names: impl IntoIterator<Item = &'a str>) -> String {
    let mut names = names
        .into_iter()
        .map(str::trim)
        .filter(|other| !other.is_empty())
        .collect::<BTreeSet<_>>();
    names.insert(name.trim());
    resolve_all(&names).remove(name.trim()).unwrap_or_default()
}

/// Deterministic, distinct badges for a set of distinct names.
fn resolve_all<'a>(names: &BTreeSet<&'a str>) -> BTreeMap<&'a str, String> {
    let shapes = names
        .iter()
        .map(|name| (*name, Shape::new(name)))
        .collect::<BTreeMap<_, _>>();
    let mut extra = names
        .iter()
        .map(|name| (*name, 0))
        .collect::<BTreeMap<_, _>>();
    loop {
        let mut groups = BTreeMap::<String, Vec<&str>>::new();
        for name in names {
            groups
                .entry(shapes[name].text(extra[name]))
                .or_default()
                .push(name);
        }
        let mut extended = false;
        let mut exhausted = Vec::new();
        for members in groups.values().filter(|members| members.len() > 1) {
            let mut grown = false;
            for name in members {
                if extra[name] < shapes[name].extension.len() {
                    *extra.get_mut(name).unwrap() += 1;
                    grown = true;
                }
            }
            if grown {
                extended = true;
            } else {
                exhausted.push(members.clone());
            }
        }
        if extended {
            continue;
        }
        let mut badges = names
            .iter()
            .map(|name| (*name, shapes[name].text(extra[name])))
            .collect::<BTreeMap<_, _>>();
        // Names that differ only in separators or case cannot be told apart by
        // letters, so number them in their stable sorted order.
        for members in exhausted {
            for (index, name) in members.into_iter().enumerate() {
                badges
                    .get_mut(name)
                    .unwrap()
                    .push_str(&(index + 1).to_string());
            }
        }
        return badges;
    }
}

struct Shape {
    base: String,
    extension: Vec<char>,
}

impl Shape {
    fn new(name: &str) -> Self {
        let words = words(name);
        let Some(first) = words.first() else {
            return Self {
                base: String::new(),
                extension: Vec::new(),
            };
        };
        if first.iter().copied().any(wide) && words.len() == 1 {
            // Scripts without word initials read best as their leading characters.
            return Self {
                base: first.iter().take(2).collect(),
                extension: first.iter().skip(2).copied().collect(),
            };
        }
        let (base, last): (Vec<char>, &[char]) = if words.len() == 1 {
            (
                first.iter().take(2).copied().collect(),
                &first[2.min(first.len())..],
            )
        } else {
            let used = &words[..words.len().min(3)];
            (
                used.iter().map(|word| word[0]).collect(),
                &used[used.len() - 1][1..],
            )
        };
        let limit = if base.iter().copied().any(wide) { 2 } else { 3 };
        Self {
            base: base
                .into_iter()
                .take(limit)
                .flat_map(char::to_uppercase)
                .collect(),
            extension: last
                .iter()
                .flat_map(|letter| letter.to_lowercase())
                .collect(),
        }
    }

    fn text(&self, extra: usize) -> String {
        let mut text = self.base.clone();
        text.extend(&self.extension[..extra]);
        text
    }
}

/// Splits on separators and camel-case boundaries: "WorkBuddy China" -> Work, Buddy, China.
fn words(name: &str) -> Vec<Vec<char>> {
    let mut words = Vec::new();
    for part in name.split(|letter: char| {
        letter.is_whitespace() || matches!(letter, '-' | '_' | '.' | '/' | '(' | ')')
    }) {
        let letters = part.chars().collect::<Vec<_>>();
        let mut start = 0;
        for index in 1..letters.len() {
            let previous = letters[index - 1];
            let current = letters[index];
            let next = letters.get(index + 1).copied();
            let camel = previous.is_lowercase() && current.is_uppercase();
            let acronym = previous.is_uppercase()
                && current.is_uppercase()
                && next.is_some_and(char::is_lowercase);
            if camel || acronym {
                words.push(letters[start..index].to_vec());
                start = index;
            }
        }
        if start < letters.len() {
            words.push(letters[start..].to_vec());
        }
    }
    words
}

/// East Asian ideographs and syllables occupy roughly two Latin letters.
fn wide(letter: char) -> bool {
    matches!(
        letter as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x3FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn alone(name: &str) -> String {
        resolve(name, [])
    }

    #[test]
    fn uses_word_initials() {
        assert_eq!(alone("WorkBuddy China"), "WBC");
        assert_eq!(alone("WorkBuddy International"), "WBI");
        assert_eq!(alone("chatgpt oauth"), "CO");
        assert_eq!(alone("open_router.ai"), "ORA");
        assert_eq!(alone("my-very-long-channel-name"), "MVL");
        assert_eq!(alone("OpenAIProxy"), "OAP");
    }

    #[test]
    fn uses_two_letters_for_single_words() {
        assert_eq!(alone("grok"), "GR");
        assert_eq!(alone("rrwkj"), "RR");
        assert_eq!(alone("x"), "X");
    }

    #[test]
    fn handles_wide_and_empty_names() {
        assert_eq!(alone("深度求索"), "深度");
        assert_eq!(alone("通义 千问 国际"), "通千");
        assert_eq!(alone("Kimi 中国"), "K中");
        assert_eq!(alone(""), "");
        assert_eq!(alone("  -_ "), "");
    }

    #[test]
    fn extends_colliding_badges() {
        let names = ["WorkBuddy China", "WorkBuddy Cloud", "grok", "groq"];
        let badge = |name| resolve(name, names);
        assert_eq!(badge("WorkBuddy China"), "WBCh");
        assert_eq!(badge("WorkBuddy Cloud"), "WBCl");
        assert_eq!(badge("grok"), "GRok");
        assert_eq!(badge("groq"), "GRoq");
    }

    #[test]
    fn keeps_unique_badges_short() {
        let names = [
            "WorkBuddy China",
            "WorkBuddy International",
            "chatgpt oauth",
        ];
        assert_eq!(resolve("WorkBuddy China", names), "WBC");
        assert_eq!(resolve("WorkBuddy International", names), "WBI");
    }

    #[test]
    fn numbers_indistinguishable_names() {
        let names = ["Open AI", "open-ai"];
        assert_eq!(resolve("Open AI", names), "OAi1");
        assert_eq!(resolve("open-ai", names), "OAi2");
    }
}
