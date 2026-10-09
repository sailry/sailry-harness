use super::*;
use sailry_protocol::{Command, web};

#[test]
fn preserves_pages_and_captured_scope() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let scope = context();
    let result = run(
        bundle(
            r#"
            import {fetchWeb,prepareRequest,completeRequest} from 'sailry/sdk';
            export async function run(input) {
                const first=await fetchWeb({url:input.url});
                const last=await fetchWeb({url:input.url,offset:first.next_offset});
                const cancelled=await completeRequest(prepareRequest({kind:'cancel_web_fetch',data:{request:input.request}}));
                return {content:first.content+last.content,last:last.next_offset,
                    untrusted:first.untrusted&&last.untrusted,cancelled};
            }
            "#,
        ),
        "run",
        json!({"url":"https://example.com/notes","request":sailry_protocol::RequestId::new()}),
        scope.clone(),
        NodeId([1; 32]),
        CancellationToken::new(),
        move |request| {
            requests.lock().unwrap().push(request.clone());
            match request.command {
                Command::FetchWeb(input) => Ok(Output::WebPage(web::Page {
                    url: input.url,
                    status: 200,
                    content_type: "text/plain".into(),
                    content: if input.offset == 0 { "中文" } else { "🙂" }.into(),
                    title: None,
                    next_offset: (input.offset == 0).then_some(2),
                    truncated: false,
                    untrusted: true,
                })),
                Command::CancelWebFetch { request } => Ok(Output::WebFetchCancelled { request }),
                _ => panic!("web command expected"),
            }
        },
    )
    .unwrap();
    assert_eq!(result["content"], "中文🙂");
    assert!(result["last"].is_null());
    assert_eq!(result["untrusted"], true);
    assert_eq!(result["cancelled"]["Ok"]["kind"], "web_fetch_cancelled");
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.plugin.as_ref() == Some(&scope))
    );
    assert!(matches!(&requests[0].command, Command::FetchWeb(input) if input.offset == 0));
    assert!(matches!(&requests[1].command, Command::FetchWeb(input) if input.offset == 2));
    assert_ne!(requests[0].id, requests[1].id);
}

#[test]
fn rejects_credential_and_scope_injection() {
    let result = run(
        bundle(
            r#"
            import {fetchWeb} from 'sailry/sdk';
            export async function run() {
                const errors=[];
                for (const extra of ['headers','credential','method','plugin','worktree']) {
                    try { await fetchWeb({url:'https://example.com/',[extra]:'private'}); }
                    catch (error) { errors.push(error.code); }
                }
                return errors;
            }
            "#,
        ),
        "run",
        json!(null),
        context(),
        NodeId([1; 32]),
        CancellationToken::new(),
        |_| panic!("invalid web requests must not dispatch"),
    )
    .unwrap();
    assert_eq!(result, json!(vec!["invalid_request"; 5]));
}
