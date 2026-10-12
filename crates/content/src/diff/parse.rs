use gpui_kit::component::diff::{DiffFile, DiffParseError};

/// Add file headers for captured tool hunks and new-file bodies. Public Git
/// patches already carry their own paths and extended metadata.
pub fn patch(text: &str, path: &str, added: bool) -> String {
    let path = if path.is_empty() { "content.txt" } else { path };
    let before = serde_json::to_string(&format!("a/{path}")).unwrap();
    let after = serde_json::to_string(&format!("b/{path}")).unwrap();
    if added {
        let count = text.lines().count();
        let mut patch = format!(
            "--- /dev/null\n+++ {after}\n@@ -0,0 +{},{count} @@\n",
            usize::from(count > 0)
        );
        for line in text.split_terminator('\n') {
            patch.push('+');
            patch.push_str(line);
            patch.push('\n');
        }
        if !text.is_empty() && !text.ends_with('\n') {
            patch.push_str("\\ No newline at end of file\n");
        }
        return patch;
    }
    if text
        .lines()
        .any(|line| line.starts_with("diff --git ") || line.starts_with("--- "))
    {
        text.to_owned()
    } else {
        format!("--- {before}\n+++ {after}\n{text}")
    }
}

/// A truncated Node patch contains only available source rows. Normalize the
/// last hunk's advertised lengths without fabricating omitted context.
pub fn files(text: &str, path: &str, added: bool) -> Result<Vec<DiffFile>, DiffParseError> {
    let patch = patch(text, path, added);
    match DiffFile::parse(&patch) {
        Ok(files) => Ok(files),
        Err(original) => {
            let lines: Vec<_> = patch.lines().collect();
            let Some(start) = lines.iter().rposition(|line| line.starts_with("@@ ")) else {
                return Err(original);
            };
            let header = lines[start];
            let Some((coordinates, suffix)) = header
                .strip_prefix("@@ ")
                .and_then(|value| value.split_once(" @@"))
            else {
                return Err(original);
            };
            let mut coordinates = coordinates.split_whitespace();
            let number = |value: Option<&str>, prefix: char| -> Option<(usize, usize)> {
                let mut fields = value?.strip_prefix(prefix)?.split(',');
                Some((
                    fields.next()?.parse().ok()?,
                    fields.next().map_or(Some(1), |value| value.parse().ok())?,
                ))
            };
            let Some((before, expected_old)) = number(coordinates.next(), '-') else {
                return Err(original);
            };
            let Some((after, expected_new)) = number(coordinates.next(), '+') else {
                return Err(original);
            };
            let tail = &lines[start + 1..];
            if tail
                .iter()
                .any(|line| !line.starts_with([' ', '+', '-', '\\']))
            {
                return Err(original);
            }
            let old = tail
                .iter()
                .filter(|line| line.starts_with([' ', '-']))
                .count();
            let new = tail
                .iter()
                .filter(|line| line.starts_with([' ', '+']))
                .count();
            if old > expected_old
                || new > expected_new
                || (old == expected_old && new == expected_new)
            {
                return Err(original);
            }
            let normalized = lines[..start]
                .iter()
                .copied()
                .chain(std::iter::once(""))
                .collect::<Vec<_>>()
                .join("\n")
                + &format!("@@ -{before},{old} +{after},{new} @@{suffix}\n")
                + &tail.join("\n");
            DiffFile::parse(&normalized).map_err(|_| original)
        }
    }
}
