use super::*;
use sailry_protocol::{Command, knowledge, plugin::storage};

#[test]
fn preserves_queries_and_bounds_read_pages() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let result = run(
        bundle(
            r#"
        import {listKeys,searchValues,queryKnowledge} from 'sailry/sdk';
        export async function run() {
            const query='q'.repeat(8193),input={query,terms:[query],limit:100000};
            await listKeys('',null,100000);
            await searchValues(input);
            await queryKnowledge({query,limit:100000,offset:10001});
            return {query:input.query,limit:input.limit};
        }
    "#,
        ),
        "run",
        json!(null),
        context(),
        NodeId([1; 32]),
        CancellationToken::new(),
        move |request| {
            requests.lock().unwrap().push(request.clone());
            Ok(match request.command {
                Command::ListPluginKeys { limit, .. } => {
                    assert_eq!(limit, storage::MAX_PAGE_LIMIT);
                    Output::PluginKeys(storage::Page {
                        keys: vec![],
                        after: None,
                    })
                }
                Command::SearchPluginValues(query) => {
                    assert_eq!(query.limit, storage::MAX_PAGE_LIMIT);
                    assert_eq!(query.query.as_ref().unwrap().len(), 8193);
                    assert_eq!(query.terms[0], query.query.as_ref().unwrap().as_str());
                    Output::PluginSearch(storage::SearchPage {
                        entries: vec![],
                        next: None,
                        now_ms: 0,
                        mode: storage::Mode::Keyword,
                        indexing: false,
                    })
                }
                Command::QueryKnowledge(query) => {
                    assert_eq!(query.limit, knowledge::PAGE_LIMIT);
                    assert_eq!(query.query.len(), 8193);
                    assert_eq!(query.offset, 10001);
                    Output::KnowledgeSearch(knowledge::Page {
                        hits: vec![],
                        next_offset: None,
                        mode: knowledge::Mode::Keyword,
                        indexing: false,
                        model: None,
                        untrusted: true,
                    })
                }
                _ => panic!("read command expected"),
            })
        },
    )
    .unwrap();
    assert_eq!(result["query"].as_str().unwrap().len(), 8193);
    assert_eq!(result["limit"], 100000);
    assert_eq!(captured.lock().unwrap().len(), 3);
}

#[test]
fn rejects_invalid_page_types_before_dispatch() {
    let result=run(bundle(r#"
        import {searchValues} from 'sailry/sdk';
        export async function run() {
            let rejected=0;
            for (const limit of [0,null,1.5,Number.MAX_SAFE_INTEGER+1]) {
                try {await searchValues({terms:['query'],limit});}
                catch (error) {if (error.message==='limit must be a positive safe integer') rejected++;}
            }
            return rejected;
        }
    "#),"run",json!(null),context(),NodeId([1;32]),CancellationToken::new(),|_|panic!("invalid read reached dispatch")).unwrap();
    assert_eq!(result, 4);
}
