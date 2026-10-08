use sailry_protocol::Fault;

pub(super) fn words(command: &str) -> Option<Vec<&str>> {
    if command.is_empty()
        || command.chars().any(|character| {
            character.is_control() && character != '\t'
                || character.is_whitespace() && !matches!(character, ' ' | '\t')
                || matches!(
                    character,
                    ';' | '&'
                        | '|'
                        | '<'
                        | '>'
                        | '$'
                        | '`'
                        | '\\'
                        | '\''
                        | '"'
                        | '('
                        | ')'
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | '*'
                        | '?'
                        | '~'
                        | '#'
                        | '!'
                )
        })
    {
        return None;
    }
    let words: Vec<_> = command
        .split([' ', '\t'])
        .filter(|word| !word.is_empty())
        .collect();
    let first = words.first()?;
    if first.contains('=') || first.starts_with('-') {
        return None;
    }
    Some(words)
}

pub(super) fn offered(command: &str) -> Option<String> {
    let words = words(command)?;
    Some(words[..words.len().min(2)].join(" "))
}

pub(super) fn select(command: &str, prefix: &str) -> Result<String, Fault> {
    let command =
        words(command).ok_or_else(|| Fault::invalid("command does not support a prefix rule"))?;
    let prefix = words(prefix).ok_or_else(|| Fault::invalid("invalid command prefix"))?;
    if !command.starts_with(&prefix) {
        return Err(Fault::invalid(
            "prefix must contain the command's first full words",
        ));
    }
    Ok(prefix.join(" "))
}

pub(super) fn matches(command: &str, prefix: &str) -> bool {
    words(command)
        .zip(words(prefix))
        .is_some_and(|(command, prefix)| command.starts_with(&prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_full_literal_words() {
        assert_eq!(
            offered(" cargo\ttest --lib ").as_deref(),
            Some("cargo test")
        );
        assert!(matches("cargo test --lib", "cargo test"));
        assert!(!matches("cargo tester", "cargo test"));
        assert_eq!(
            select("cargo test --lib", " cargo\ttest ").unwrap(),
            "cargo test"
        );
        assert!(select("cargo test", "test").is_err());
    }

    #[test]
    fn rejects_shell_syntax() {
        for command in [
            "cargo test; touch side",
            "cargo test && touch side",
            "cargo test | cat",
            "cargo test > out",
            "cargo test < in",
            "cargo test $(touch side)",
            "cargo test `touch side`",
            "cargo test\nother",
            "cargo test\0",
            "PATH=other cargo test",
            "'cargo' test",
            "cargo\\ test",
            "cargo test *",
            "cargo test ~/file",
            "cargo test # comment",
            "cargo test ${VAR}",
        ] {
            assert!(!matches(command, "cargo test"), "{command:?}");
        }
    }

    #[test]
    fn rejects_non_ascii_separators() {
        for character in ['\u{00a0}', '\u{2003}', '\u{2028}', '\r', '\u{000b}'] {
            let command = format!("cargo{character}test");
            assert!(words(&command).is_none(), "{command:?}");
            assert!(!matches(&command, "cargo test"));
        }
        assert!(matches("cargo\ttest", "cargo test"));
    }
}
