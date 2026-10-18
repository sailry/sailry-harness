//! Release text comes from the inspected package version and ordinary immutable resources.
use super::*;
use sailry_protocol::plugin::catalog::ReleaseNotes;
use std::collections::BTreeMap;

const LOCALES: [&str; 10] = [
    "en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "ru",
];

pub(super) fn read(
    version: Option<&str>,
    mut load: impl FnMut(&str) -> Result<Option<Vec<u8>>, Fault>,
) -> Result<Option<ReleaseNotes>, Fault> {
    let Some(version) = version else {
        return Ok(None);
    };
    let Some(text) = load("CHANGELOG.md")?
        .map(|bytes| section(&bytes, version))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let mut locales = BTreeMap::new();
    for locale in LOCALES {
        if let Some(bytes) = load(&format!("CHANGELOG.{locale}.md"))?
            && let Some(text) = section(&bytes, version)?
        {
            locales.insert(locale.into(), text);
        }
    }
    let notes = ReleaseNotes { text, locales };
    if !notes.valid()
        || serde_json::to_vec(&notes).map_err(|_| invalid())?.len()
            > sailry_protocol::MAX_FRAME_BYTES
    {
        return Err(invalid());
    }
    Ok(Some(notes))
}

fn section(bytes: &[u8], version: &str) -> Result<Option<String>, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut selected = false;
    let mut body = String::new();
    for line in text.split_inclusive('\n') {
        if let Some(heading) = line.trim_end().strip_prefix("## ") {
            if selected {
                break;
            }
            selected = heading.trim() == version;
        } else if selected {
            body.push_str(line);
        }
    }
    let text = body.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
    {
        return Err(invalid());
    }
    Ok(Some(text.into()))
}

fn invalid() -> Fault {
    Fault::invalid("invalid package release notes")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_only_the_exact_release() {
        let files = BTreeMap::from([
            (
                "CHANGELOG.md",
                "# Changes\n\n## 0.2.0\n- Future\n\n## 0.1.1\n- Fixed behavior\n\n## 0.1.0\n- Initial\n",
            ),
            ("CHANGELOG.zh-CN.md", "## 0.1.1\n- 修复行为\n"),
        ]);
        let read_version = |version| {
            read(Some(version), |path| {
                Ok(files.get(path).map(|text| text.as_bytes().to_vec()))
            })
        };
        let notes = read_version("0.1.1").unwrap().unwrap();
        assert_eq!(notes.text, "- Fixed behavior");
        assert_eq!(notes.text("zh-CN"), "- 修复行为");
        assert_eq!(notes.text("fr"), notes.text);
        assert!(read_version("0.1.1+other").unwrap().is_none());
        assert!(read_version("9.0.0").unwrap().is_none());
        assert!(section(b"## 0.1.0\nprivate\0", "0.1.0").is_err());
    }
}
