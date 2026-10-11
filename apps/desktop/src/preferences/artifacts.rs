//! Controller-owned form values, scoped to one Node and immutable document revision.
use super::*;
use sailry_protocol::{ArtifactId, NodeId};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Field {
    name: String,
    values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draft {
    pub form: String,
    fields: Vec<Field>,
}

impl Draft {
    pub(crate) fn valid(&self) -> bool {
        named(&self.form)
            && self.fields.len() <= 32
            && self.fields.iter().enumerate().all(|(index, field)| {
                named(&field.name)
                    && self.fields[..index]
                        .iter()
                        .all(|previous| previous.name != field.name)
                    && field.values.len() <= 64
                    && field
                        .values
                        .iter()
                        .all(|value| value.len() <= 4096 && !value.contains('\0'))
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 16 * 1024)
    }
}

fn named(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn scope(node: NodeId, artifact: ArtifactId) -> String {
    let node: String = node.0.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{node}/{artifact}/")
}

fn prefix(node: NodeId, artifact: ArtifactId, revision: u32) -> String {
    format!("{}{revision}/", scope(node, artifact))
}

pub(crate) fn revision(node: NodeId, artifact: ArtifactId, cx: &App) -> Option<u32> {
    let scope = scope(node, artifact);
    cx.try_global::<Preferences>()?
        .data
        .artifact_drafts
        .as_ref()?
        .iter()
        .filter(|(_, draft)| draft.valid())
        .filter_map(|(key, _)| {
            key.strip_prefix(&scope)?
                .split('/')
                .next()?
                .parse::<u32>()
                .ok()
                .filter(|revision| *revision > 0)
        })
        .max()
}

pub(crate) fn drafts(node: NodeId, artifact: ArtifactId, revision: u32, cx: &App) -> Vec<Draft> {
    let prefix = prefix(node, artifact, revision);
    cx.try_global::<Preferences>()
        .and_then(|preferences| preferences.data.artifact_drafts.as_ref())
        .map(|drafts| {
            drafts
                .iter()
                .filter(|(key, draft)| key.starts_with(&prefix) && draft.valid())
                .map(|(_, draft)| draft.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn remember(
    node: NodeId,
    artifact: ArtifactId,
    revision: u32,
    draft: Draft,
    cx: &mut App,
) -> Result<(), ()> {
    if revision == 0 || !draft.valid() {
        return Err(());
    }
    let key = format!("{}{}", prefix(node, artifact, revision), draft.form);
    if cx
        .try_global::<Preferences>()
        .and_then(|preferences| preferences.data.artifact_drafts.as_ref()?.get(&key))
        == Some(&draft)
    {
        return Ok(());
    }
    super::update(cx, |data| {
        data.artifact_drafts
            .get_or_insert_with(BTreeMap::new)
            .insert(key, draft);
    });
    Ok(())
}

pub(crate) fn forget(node: NodeId, artifact: ArtifactId, revision: u32, form: &str, cx: &mut App) {
    let key = format!("{}{form}", prefix(node, artifact, revision));
    super::update(cx, |data| {
        if let Some(drafts) = &mut data.artifact_drafts {
            drafts.remove(&key);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn retains_scoped_values_across_restart(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let node = NodeId([1; 32]);
        let artifact = ArtifactId::new();
        let draft = Draft {
            form: "review".into(),
            fields: vec![Field {
                name: "decision".into(),
                values: vec!["yes".into()],
            }],
        };
        cx.update(|cx| {
            cx.set_global(Preferences::open(path.clone()));
            remember(node, artifact, 1, draft.clone(), cx).unwrap();
            cx.set_global(Preferences::open(path.clone()));
            assert_eq!(drafts(node, artifact, 1, cx), [draft]);
            assert_eq!(revision(node, artifact, cx), Some(1));
            assert!(drafts(NodeId([2; 32]), artifact, 1, cx).is_empty());
            assert!(drafts(node, ArtifactId::new(), 1, cx).is_empty());
            assert!(drafts(node, artifact, 2, cx).is_empty());
            forget(node, artifact, 1, "review", cx);
            cx.set_global(Preferences::open(path));
            assert!(drafts(node, artifact, 1, cx).is_empty());
        });
    }

    #[test]
    fn rejects_invalid_or_unbounded_values() {
        for source in [
            r#"{"form":"review","fields":[{"name":"choice","values":["yes"]},{"name":"choice","values":["no"]}]}"#,
            r#"{"form":"../review","fields":[]}"#,
        ] {
            assert!(!serde_json::from_str::<Draft>(source).unwrap().valid());
        }
        let draft = Draft {
            form: "review".into(),
            fields: vec![Field {
                name: "note".into(),
                values: vec!["x".repeat(4097)],
            }],
        };
        assert!(!draft.valid());
    }
}
