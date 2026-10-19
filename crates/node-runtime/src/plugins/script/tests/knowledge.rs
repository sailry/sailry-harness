use super::*;
use sailry_protocol::{Command, KnowledgeId, WorktreeId, knowledge};

fn collection() -> knowledge::Collection {
    knowledge::Collection {
        id: KnowledgeId::new(),
        revision: 9_007_199_254_740_993,
        paused: false,
        rebuild: 0,
        owner: knowledge::Owner::Plugin {
            name: "example".into(),
        },
        definition: knowledge::Definition {
            name: "Notes".into(),
            source: knowledge::Source::Storage {
                scope: plugin::storage::Scope::Node,
                prefix: "notes/".into(),
            },
            model: None,
            globs: vec![],
            tags: vec![],
            shared_with: vec![],
        },
        progress: Default::default(),
    }
}

#[test]
fn captures_sources_and_exact_revisions() {
    let mut scope = context();
    scope.worktree = Some(WorktreeId::new());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let model = knowledge::Model::Registered {
        id: sailry_protocol::ModelId::new(),
    };
    let mut entry = collection();
    entry.definition.model = Some(model);
    let result = run(bundle(r#"
        import {listKnowledgeCollections,queryKnowledge,prepareKnowledgeCollection,
          prepareKnowledgeRebuild,prepareKnowledgeRemoval,completeRequest} from 'sailry/sdk';
        export async function run(input) {
          const list=await listKnowledgeCollections();
          const page=await queryKnowledge({query:'  Original query 東京  '});
          const create=prepareKnowledgeCollection({name:'Notes',source:{kind:'worktree',path:'notes'},model:input.model});
          const first=await completeRequest(create);
          const again=await completeRequest(create);
          const rebuild=await completeRequest(prepareKnowledgeRebuild(list[0].id,list[0].revision));
          const remove=await completeRequest(prepareKnowledgeRemoval(list[0].id,list[0].revision));
          return {revision:list[0].revision,created:first.Ok.data.revision,
            same:JSON.stringify(first)===JSON.stringify(again),mode:page.mode,
            rebuilt:rebuild.Ok.kind,removed:remove.Ok.kind,
            binding:list[0].definition.model};
        }
    "#), "run", json!({"model":model}), scope.clone(), NodeId([1;32]), CancellationToken::new(), move |request| {
        let output = match &request.command {
            Command::ListKnowledgeCollections => Output::KnowledgeCollections(vec![entry.clone()]),
            Command::QueryKnowledge(_) => Output::KnowledgeSearch(knowledge::Page { hits: vec![], next_offset: None, mode: knowledge::Mode::Keyword, indexing: true, model: None, untrusted: true }),
            Command::CreateKnowledgeCollection { .. } | Command::RebuildKnowledgeCollection { .. } => Output::KnowledgeCollection(entry.clone()),
            Command::DeleteKnowledgeCollection { id, .. } => Output::KnowledgeCollectionDeleted { id: *id },
            _ => panic!("knowledge command expected"),
        };
        requests.lock().unwrap().push(request);
        Ok(output)
    }).unwrap();
    assert_eq!(
        result,
        json!({"revision":"9007199254740993","created":"9007199254740993","same":true,"mode":"keyword","rebuilt":"knowledge_collection","removed":"knowledge_collection_deleted","binding":model})
    );
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(
        requests
            .iter()
            .all(|request| request.plugin.as_ref() == Some(&scope))
    );
    assert!(
        matches!(&requests[1].command, Command::QueryKnowledge(query) if query.query == "  Original query 東京  " && query.mode == knowledge::Mode::Hybrid)
    );
    assert_eq!(requests[2], requests[3]);
    assert!(
        matches!(&requests[2].command, Command::CreateKnowledgeCollection { definition, plugin: None } if definition.source == knowledge::Source::Worktree { worktree: scope.worktree.unwrap(), path: "notes".into() } && definition.shared_with.is_empty() && definition.model == Some(model))
    );
    assert!(matches!(
        requests[4].command,
        Command::RebuildKnowledgeCollection {
            expected_revision: 9_007_199_254_740_993,
            ..
        }
    ));
    assert!(matches!(
        requests[5].command,
        Command::DeleteKnowledgeCollection {
            expected_revision: 9_007_199_254_740_993,
            ..
        }
    ));
}

#[test]
fn returns_registered_search_models() {
    let model = knowledge::Model::Registered {
        id: sailry_protocol::ModelId::new(),
    };
    let result = run(
        bundle(
            r#"
            import {queryKnowledge} from 'sailry/sdk';
            export async function run() {
                const page=await queryKnowledge({query:'Original query',mode:'semantic'});
                return {model:page.model,mode:page.mode,untrusted:page.untrusted};
            }
            "#,
        ),
        "run",
        json!(null),
        context(),
        NodeId([1; 32]),
        CancellationToken::new(),
        move |request| {
            assert!(matches!(request.command, Command::QueryKnowledge(query) if query.query == "Original query" && query.mode == knowledge::Mode::Semantic));
            Ok(Output::KnowledgeSearch(knowledge::Page {
                hits: vec![],
                next_offset: None,
                mode: knowledge::Mode::Semantic,
                indexing: false,
                model: Some(model),
                untrusted: true,
            }))
        },
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"model":model,"mode":"semantic","untrusted":true})
    );
}

#[test]
fn refuses_scope_injection_and_script_write_bypass() {
    let id = KnowledgeId::new();
    let result = run(bundle(r#"
        import {prepareKnowledgeCollection,prepareKnowledgeRebuild} from 'sailry/sdk';
        export function run(input) {
          const errors=[];
          for (const draft of [
            {name:'Notes',source:{kind:'worktree',path:'notes'}},
            {name:'Notes',source:{kind:'directory',path:'/tmp'}},
            {name:'Notes',source:{kind:'storage',scope:'node',prefix:'',owner:'other'}},
            {name:'Notes',source:{kind:'storage',scope:'node',prefix:''},shared_with:['other']}
          ]) { try { prepareKnowledgeCollection(draft); errors.push(null); } catch (e) { errors.push(e.code); } }
          for (const revision of [9007199254740992,'bad']) {
            try { prepareKnowledgeRebuild(input.id,revision); errors.push(null); } catch (e) { errors.push(e.code); }
          }
          return errors;
        }
    "#), "run", json!({"id":id}), context(), NodeId([1;32]), CancellationToken::new(), |_| panic!("invalid drafts must not dispatch")).unwrap();
    assert_eq!(result, json!(vec!["invalid_request"; 6]));
    for (mode, read_only) in [
        (sailry_protocol::WorkMode::Plan, false),
        (sailry_protocol::WorkMode::Code, true),
        (sailry_protocol::WorkMode::Code, false),
    ] {
        let state = Arc::new(Mutex::new(execution::State {
            call_id: Some(sailry_protocol::RequestId::new()),
            mode,
            read_only,
            turn: None,
            staged_turn: None,
        }));
        let result = run_tool(bundle(r#"
            import {prepareKnowledgeCollection} from 'sailry/sdk';
            export function run() {
              try { prepareKnowledgeCollection({name:'Notes',source:{kind:'storage',scope:'node',prefix:'notes/'}}); return null; }
              catch (error) { return error.code; }
            }
        "#),"run",json!(null),Environment {context:context(),target:NodeId([1;32]),stop:CancellationToken::new(),
 deadline: Instant::now() + CALL_TIMEOUT,
},|_| panic!("script writes must not bypass native approval"),state).unwrap();
        assert_eq!(result, "permission_denied");
    }
}
