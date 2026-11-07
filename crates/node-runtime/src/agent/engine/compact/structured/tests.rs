//! Derived from Goose 14741e9a, goose-context-management/src/structured.rs tests.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::*;

const RESPONSE: &str = r#"<analysis>
The user asked to fix a bug in parser.rs. I traced it to an off-by-one
in {brace handling} and patched it.
</analysis>

```json
{
  "user_intent": ["Fix the parser bug", "Add a regression test"],
  "technical_concepts": ["off-by-one", "tokenizer"],
  "files": [
    {"path": "src/parser.rs", "summary": "Fixed off-by-one in scan loop", "key_code": "fn scan(&mut self) { .. }"}
  ],
  "errors_and_fixes": ["Panic on empty input, fixed with early return"],
  "problem_solving": ["Root-caused via failing unit test"],
  "user_messages": ["fix the parser bug", "add a test"],
  "pending_tasks": ["Add a regression test"],
  "current_work": "Writing the regression test in tests/parser.rs",
  "next_step": "Finish the regression test"
}
```"#;

#[test]
fn parses_fenced_json_after_analysis() {
    let summary = Summary::parse(RESPONSE).unwrap();
    assert_eq!(
        summary.user_intent,
        ["Fix the parser bug", "Add a regression test"]
    );
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].path, "src/parser.rs");
    assert_eq!(
        summary.current_work.as_deref(),
        Some("Writing the regression test in tests/parser.rs")
    );
}

#[test]
fn rejects_unusable_documents() {
    for text in [
        "Here is a summary of the conversation. The user asked about compaction.",
        "{}",
        r#"{"notes": "unknown fields alone are not a summary"}"#,
        r#"{"current_work": ""}"#,
        r#"{"files": [{}], "user_intent": [" "]}"#,
        "```json\n{\"user_intent\": [\"Fix the bug\"], \"pending_tasks\": [\"Write tests\", \"Update docs",
        r#"The session focused on the parser migration. The tracker entry {"current_work": "migrate parser"} is unchanged, and tests still need porting."#,
        "<analysis>reviewing</analysis>\nA prose recap: the config was set to {\"user_intent\": [\"quoted example\"]} per the docs, then the run passed.",
        "<analysis>\nThe target shape is:\n```json\n{\"user_intent\": [\"example only\"]}\n```\nNow let me review the conversation.\n</analysis>\nSorry, I ran out of room and could not produce the summary document.",
        "<analysis>\nThe prompt ends with </analysis> and shows the shape:\n```json\n{\"user_intent\": [\"example only\"]}\n```\nNow let me review the conversation.\n</analysis>\nSorry, I ran out of room and could not produce the summary document.",
    ] {
        assert!(Summary::parse(text).is_none(), "{text}");
    }
}

#[test]
fn keeps_embedded_fences() {
    let text = "```json\n{\"user_intent\": [\"Document the build\"], \"files\": [{\"path\": \"README.md\", \"summary\": \"Added build docs\", \"key_code\": \"```bash\\ncargo build\\n```\"}], \"pending_tasks\": [\"Publish the docs\"]}\n```";
    let summary = Summary::parse(text).unwrap();
    assert_eq!(
        summary.files[0].key_code.as_deref(),
        Some("```bash\ncargo build\n```")
    );
    assert_eq!(summary.pending_tasks, ["Publish the docs"]);

    let quoted = "```json\n{\"user_intent\": [\"Document the config\"], \"files\": [{\"path\": \"docs/config.md\", \"summary\": \"Added config examples\", \"key_code\": \"```json\\n{\\\"retries\\\": 3}\\n```\"}]}\n```";
    let summary = Summary::parse(quoted).unwrap();
    assert_eq!(
        summary.files[0].key_code.as_deref(),
        Some("```json\n{\"retries\": 3}\n```")
    );
}

#[test]
fn keeps_quoted_analysis_terminator() {
    let text = "<analysis>\nThe session edited the compaction prompt itself.\n</analysis>\n```json\n{\"user_intent\": [\"Rework the scratchpad prompt\"], \"files\": [{\"path\": \"prompts/compact.md\", \"summary\": \"Tightened the <analysis>...</analysis> instructions\"}]}\n```";
    let summary = Summary::parse(text).unwrap();
    assert_eq!(summary.user_intent, ["Rework the scratchpad prompt"]);
    assert_eq!(
        summary.files[0].summary,
        "Tightened the <analysis>...</analysis> instructions"
    );
}

#[test]
fn tries_next_candidate_after_broken_fence() {
    let text = "<analysis>the model was told to emit ```json with {braces</analysis>\n{\"user_intent\": [\"Real goal\"]}";
    let summary = Summary::parse(text).unwrap();
    assert_eq!(summary.user_intent, ["Real goal"]);
}

#[test]
fn accepts_lenient_fields() {
    let text = r#"{
        "user_intent": "fix the flaky test",
        "errors_and_fixes": [
            {"error": "cursor drifted after replay batch 34", "fix": "bounded mpsc channel"},
            "plain string entry",
            null
        ],
        "pending_tasks": [42],
        "current_work": {"task": "regression test", "status": "in progress"}
    }"#;
    let summary = Summary::parse(text).unwrap();
    assert_eq!(summary.user_intent, ["fix the flaky test"]);
    assert_eq!(
        summary.errors_and_fixes,
        [
            "error: cursor drifted after replay batch 34; fix: bounded mpsc channel",
            "plain string entry",
        ]
    );
    assert_eq!(summary.pending_tasks, ["42"]);
    assert_eq!(
        summary.current_work.as_deref(),
        Some("task: regression test; status: in progress")
    );
}

#[test]
fn accepts_lenient_file_entries() {
    let text = r#"{"files": [
        "src/parser.rs",
        {"path": "tests/parser.rs", "summary": "Added regression test"},
        {"path": "src/scan.rs", "summary": 42, "key_code": ["fn a() {}", "fn b() {}"]},
        ""
    ]}"#;
    let summary = Summary::parse(text).unwrap();
    assert_eq!(summary.files.len(), 3);
    assert_eq!(summary.files[0].path, "src/parser.rs");
    assert_eq!(summary.files[0].summary, "");
    assert_eq!(summary.files[1].summary, "Added regression test");
    assert_eq!(summary.files[2].summary, "42");
    assert_eq!(
        summary.files[2].key_code.as_deref(),
        Some("fn a() {}; fn b() {}")
    );
}

#[test]
fn drops_blank_entries() {
    let text = r#"{"user_intent": ["", "Fix the bug"], "files": [{"path": "a.rs", "summary": "Patched", "key_code": "  "}], "next_step": " "}"#;
    let summary = Summary::parse(text).unwrap();
    assert_eq!(summary.user_intent, ["Fix the bug"]);
    assert_eq!(summary.files[0].key_code, None);
    assert_eq!(summary.next_step, None);
}

#[test]
fn accepts_unescaped_control_characters() {
    let summary = Summary::parse("{\"user_intent\":[\"Fix\nparser\"]}").unwrap();
    assert_eq!(summary.user_intent, ["Fix\nparser"]);
}

#[test]
fn renders_handoff_sections() {
    let summary = Summary::parse(RESPONSE).unwrap();
    let rendered = summary.render().unwrap();
    assert!(rendered.starts_with("# Handoff Summary\n\n## Goal\n"));
    assert!(rendered.contains("- Fix the parser bug"));
    assert!(rendered.contains("- `src/parser.rs`: Fixed off-by-one in scan loop"));
    assert!(rendered.contains("fn scan(&mut self) { .. }"));
    assert!(rendered.contains("## Next Steps"));
}

#[test]
fn code_fences_exceed_embedded_runs() {
    let summary = Summary {
        files: vec![File {
            path: "docs/build.md".to_owned(),
            summary: "Documented the build".to_owned(),
            key_code: Some("```bash\ncargo build\n```\n````\nnested fence docs\n````".to_owned()),
        }],
        errors_and_fixes: vec!["None".to_owned()],
        ..Default::default()
    };
    let rendered = summary.render().unwrap();
    assert_eq!(rendered.matches("\n`````\n").count(), 2);
    let error = rendered.find("- None").unwrap();
    let closing = rendered.rfind("\n`````\n").unwrap();
    assert!(error > closing);
}
