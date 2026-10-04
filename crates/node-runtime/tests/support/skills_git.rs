//! A real isolated Git repository; acquisition uses the production Git transport.
use sailry_protocol::{
    Command,
    plugin::skills::{Candidate, Discovery, Source},
};
use std::collections::BTreeMap;

pub struct Git {
    directory: tempfile::TempDir,
    pub url: String,
    pub first: String,
}

impl Git {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = git2::Repository::init_bare(directory.path()).unwrap();
        let first = commit(&repository, "first", None);
        repository.set_head("refs/heads/release/v1").unwrap();
        let url = url::Url::from_file_path(directory.path())
            .unwrap()
            .to_string();
        Self {
            directory,
            url,
            first,
        }
    }

    pub fn update(&self) -> String {
        let repository = git2::Repository::open_bare(self.directory.path()).unwrap();
        let parent = repository.head().unwrap().peel_to_commit().unwrap();
        commit(&repository, "second", Some(&parent))
    }

    pub fn source(&self) -> Source {
        Source {
            repository: self.url.clone(),
            git_ref: Some("release/v1".into()),
            path: None,
        }
    }
}

pub fn install(discovery: &Discovery, skill: &Candidate, revision: u64) -> Command {
    Command::InstallSkill {
        source: discovery.source.clone(),
        path: skill.path.clone(),
        name: skill.name.clone(),
        expected_revision: revision,
    }
}

pub fn body(version: &str) -> String {
    format!(
        "---\nname: analysis\ndescription: Analyze project data\n---\nVersion {version}\nRead references/guide.md and scripts/check.sh\nLiteral {{missing_state}} 中文 🙂\n"
    )
}

fn commit(
    repository: &git2::Repository,
    version: &str,
    parent: Option<&git2::Commit<'_>>,
) -> String {
    let entries = [
        ("LICENSE", "Fixture license notice".into(), 0o100644),
        ("skills/analysis/SKILL.md", body(version), 0o100644),
        ("skills/analysis/references/guide.md", format!("Guide {version} 中文 🙂"), 0o100644),
        ("skills/analysis/scripts/check.sh", format!("printf 'script {version}'; printf x >> script-count.txt\n"), 0o100755),
        ("skills/analysis/assets/example.txt", "Asset content".into(), 0o100644),
        ("skills/analysis/references/example/SKILL.md", "Example document is not an install candidate".into(), 0o100644),
        ("other/SKILL.md", "---\nname: writing\ndescription: Write clearly\n---\nKeep text concise\n".into(), 0o100644),
        ("unsafe/SKILL.md", "---\nname: unsafe\ndescription: A skill with an unavailable resource\n---\nRead references/secret\n".into(), 0o100644),
        ("CLAUDE.md", "AGENTS.md".into(), 0o120000),
        ("unsafe/references/secret", "../../../secret".into(), 0o120000),
        ("plugin/plugin.json", serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"generic-plugin","version":version}).to_string(), 0o100644),
        ("plugin/README.md", format!("Plugin {version}\n"), 0o100644),
        ("plugin/LICENSE", "Plugin fixture license".into(), 0o100644),
    ];
    let tree = tree(
        repository,
        &entries
            .iter()
            .map(|(path, text, mode)| {
                (
                    (*path).to_owned(),
                    repository.blob(text.as_bytes()).unwrap(),
                    *mode,
                )
            })
            .collect::<Vec<_>>(),
    );
    let tree = repository.find_tree(tree).unwrap();
    let signature = git2::Signature::now("Skill fixture", "skill@example.invalid").unwrap();
    repository
        .commit(
            Some("refs/heads/release/v1"),
            &signature,
            &signature,
            version,
            &tree,
            &parent.into_iter().collect::<Vec<_>>(),
        )
        .unwrap()
        .to_string()
}

fn tree(repository: &git2::Repository, entries: &[(String, git2::Oid, i32)]) -> git2::Oid {
    let mut builder = repository.treebuilder(None).unwrap();
    let mut children = BTreeMap::<String, Vec<(String, git2::Oid, i32)>>::new();
    for (path, oid, mode) in entries {
        if let Some((parent, child)) = path.split_once('/') {
            children
                .entry(parent.into())
                .or_default()
                .push((child.into(), *oid, *mode));
        } else {
            builder.insert(path, *oid, *mode).unwrap();
        }
    }
    for (name, entries) in children {
        builder
            .insert(&name, tree(repository, &entries), 0o040000)
            .unwrap();
    }
    builder.write().unwrap()
}
