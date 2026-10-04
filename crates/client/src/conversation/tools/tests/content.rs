use super::*;
use sailry_protocol::{
    AttachmentId, WorktreeId, attachment, conversation::Image, tool::Presentation,
};

fn fixture() -> Page {
    let mut page = page();
    if let Part::ToolCall {
        presentation,
        arguments,
        ..
    } = &mut page.entries[0].parts[0]
    {
        *presentation = Presentation::Content;
        *arguments = json!({"presentation":"summary"});
    }
    let mut result = entry(2, page.runs[0].turn, "", true);
    result.parts[0] = Part::ToolResult {
        id: Some("call".into()),
        name: "read_file".into(),
        result: json!({"output":{"sailry_content":{"version":1,"blocks":[
            {"kind":"table","columns":["Name"],"rows":[["中文 🙂"]]},
            {"kind":"image","index":2}
        ]}}}),
        images: vec![Image {
            entry: result.id.clone(),
            part: 0,
            index: 2,
            attachment: attachment::Attachment {
                id: AttachmentId::new(),
                spec: attachment::Spec {
                    worktree: WorktreeId::new(),
                    name: "image.png".into(),
                    media_type: "image/png".into(),
                    size: 5,
                    revision: "a".repeat(64),
                },
            },
        }],
    };
    page.entries.push(result);
    page
}

#[test]
fn restores() {
    let page = fixture();
    let calls = collect(&page);
    assert_eq!(calls[0].content.as_ref().unwrap().blocks.len(), 2);
    let restored: Page = serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
    assert_eq!(collect(&restored), calls);
    assert_eq!(
        serde_json::to_value(&calls).unwrap()[0]["content"]["version"],
        1
    );
    assert!(calls[0].result(&page).unwrap()["output"]["sailry_content"].is_object());
}

#[test]
fn association() {
    let original = fixture();
    for change in 0..9 {
        let mut page = original.clone();
        match change {
            0 => {
                page.entries.remove(0);
            }
            1 => page.entries[1].turn = TurnId::new(),
            2 => page.entries[1].branch = "other".into(),
            3 => {
                if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
                    *presentation = Presentation::Details;
                }
            }
            4 => {
                if let Part::ToolResult { name, .. } = &mut page.entries[1].parts[0] {
                    *name = "different".into();
                }
            }
            5 => {
                if let Part::ToolResult { images, .. } = &mut page.entries[1].parts[0] {
                    images.clear();
                }
            }
            6 => {
                if let Part::ToolResult { images, .. } = &mut page.entries[1].parts[0] {
                    images[0].index = 1;
                }
            }
            7 => {
                if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
                    result["error"] = json!("failed");
                }
            }
            8 => {
                if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
                    result["output"]["sailry_content"]["version"] = json!(2);
                }
            }
            _ => unreachable!(),
        }
        let calls = collect(&page);
        assert!(
            calls.iter().all(|call| call.content.is_none()),
            "change {change}"
        );
        assert!(calls.iter().any(|call| call.result(&page).is_some()));
    }
}
