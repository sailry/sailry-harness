use super::*;

pub(super) fn response(path: &str, status: u16) -> Option<(u16, &'static str, Vec<u8>)> {
    let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    let json = match url.path() {
        "/skills-sh/api/skills/all-time/0" => {
            serde_json::json!({"skills":[{"source":"example/foreign","skillId":"review","name":"Review"},{"source":"open.feishu.cn","skillId":"lark-doc","name":"lark-doc"}],"page":0,"total":400,"hasMore":true})
        }
        "/skills-sh/api/skills/all-time/1" => {
            serde_json::json!({"skills":[{"source":"example/foreign","skillId":"analysis","name":"Analysis"}],"page":1,"total":400,"hasMore":false})
        }
        "/skills-sh/api/search" => {
            assert_eq!(query.get("q").unwrap(), "review & analysis");
            assert_eq!(query.get("limit").unwrap(), "100");
            serde_json::json!({"skills":[{"source":"example/foreign","skillId":"review","name":"Review"},{"source":"open.feishu.cn","skillId":"lark-doc","name":"lark-doc"}],"count":2})
        }
        "/api/v1/plugins/foreign" | "/api/v1/plugins/invalid" => {
            let name = url.path().rsplit('/').next().unwrap();
            serde_json::json!({"data":{"repoUrl":format!("https://github.com/example/{name}"),"pluginPath":"","manifests":{
                "agent-plugins":{"path":"plugin.json","raw":"{\"$schema\":\"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json\",\"name\":\"synthetic-name\"}"},
                "codex":{"path":".codex-plugin/plugin.json","raw":"{}"}
            }}})
        }
        "/repos/example/foreign/commits/HEAD" | "/repos/example/invalid/commits/HEAD" => {
            return Some((200, "application/vnd.github.sha", FIRST.as_bytes().to_vec()));
        }
        value
            if value == format!("/example/foreign/zip/{FIRST}")
                || value == format!("/example/invalid/zip/{FIRST}") =>
        {
            return Some((200, "application/zip", archive(value.contains("/invalid/"))));
        }
        _ => return None,
    };
    Some((
        status,
        "application/json",
        serde_json::to_vec(&json).unwrap(),
    ))
}

fn archive(invalid: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    let manifest = if invalid {
        r#"{"$schema":"invalid","name":"foreign"}"#
    } else {
        r#"{"name":"foreign"}"#
    };
    for (path, body) in [
        ("plugin.json", manifest.to_owned()),
        (".codex-plugin/plugin.json", r#"{"name":"foreign","version":"1.2.3","description":"Foreign skills","hooks":{"danger":"must never execute"}}"#.to_owned()),
        ("LICENSE", "Fixture license".to_owned()),
        ("skills/review/SKILL.md", "---\nname: review\ndescription: Review changes\n---\nReview supplied changes\n".to_owned()),
        ("skills/analysis/SKILL.md", "---\nname: analysis\ndescription: Analyze changes\n---\nAnalyze supplied changes\n".to_owned()),
    ] {
        zip.start_file(format!("repo-source/{path}"), options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
