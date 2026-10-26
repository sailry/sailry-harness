//! Display aliases come from the execution Node; authoritative arguments stay intact.
use sailry_protocol::tool::{Alias, AliasKind};

pub(super) fn format(command: &str, aliases: &[Alias]) -> String {
    let mut aliases: Vec<_> = aliases
        .iter()
        .filter(|alias| !alias.path.is_empty())
        .collect();
    aliases.sort_by_key(|alias| std::cmp::Reverse(alias.path.len()));
    let mut output = String::with_capacity(command.len());
    let mut position = 0;
    while position < command.len() {
        let remaining = &command[position..];
        let previous = command[..position].chars().next_back();
        let alias = aliases.iter().find(|alias| {
            remaining.starts_with(&alias.path)
                && previous.is_none_or(delimiter)
                && remaining[alias.path.len()..]
                    .chars()
                    .next()
                    .is_none_or(|next| {
                        delimiter(next)
                            || alias.kind == AliasKind::Home && matches!(next, '/' | '\\')
                    })
        });
        if let Some(alias) = alias {
            output.push_str(match alias.kind {
                AliasKind::Home => "~",
                AliasKind::Python => "python",
            });
            position += alias.path.len();
        } else {
            let character = remaining.chars().next().unwrap();
            output.push(character);
            position += character.len_utf8();
        }
    }
    output
}

fn delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '\'' | '"' | '=' | ';' | '|' | '&' | '(' | ')' | '<' | '>'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_path_and_token_boundaries() {
        let aliases = [
            Alias {
                path: "/Users/fixture".into(),
                kind: AliasKind::Home,
            },
            Alias {
                path: "/Users/fixture/node/python/bin/python3".into(),
                kind: AliasKind::Python,
            },
        ];
        assert_eq!(
            format(
                "'/Users/fixture/node/python/bin/python3' /Users/fixture/task.py",
                &aliases
            ),
            "'python' ~/task.py"
        );
        assert_eq!(
            format(
                "/Users/fixture2/task.py /tmp/Users/fixture /Users/fixture/node/python/bin/python3-other",
                &aliases
            ),
            "/Users/fixture2/task.py /tmp/Users/fixture ~/node/python/bin/python3-other"
        );
        assert_eq!(
            format(
                "printf /other/.sailry/python/bin/python /other/home",
                &aliases
            ),
            "printf /other/.sailry/python/bin/python /other/home"
        );
    }

    #[test]
    fn formats_windows_and_unicode_paths() {
        let aliases = [Alias {
            path: "C:\\Users\\fixture".into(),
            kind: AliasKind::Home,
        }];
        assert_eq!(
            format("type \"C:\\Users\\fixture\\文档.txt\"", &aliases),
            "type \"~\\文档.txt\""
        );
    }
}
