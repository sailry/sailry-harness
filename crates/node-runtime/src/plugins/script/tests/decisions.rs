use super::*;
use sailry_protocol::{Command, decision};

#[test]
fn preserves_questions_and_scoped_answers() {
    let expected = context();
    let scope = expected.clone();
    let result = run(
        bundle(r#"
            import {decide} from 'sailry/decisions';
            export async function run() {
                return decide({state:'Save is visible',questions:{save:{type:'noul',instructions:'Is Save visible?'}}});
            }
        "#),
        "run", json!(null), scope, NodeId([1;32]), CancellationToken::new(),
        move |request| {
            assert_eq!(request.plugin, Some(expected.clone()));
            let Command::Decide(input) = request.command else { panic!("decision expected") };
            assert_eq!(input.model, None);
            assert_eq!(input.state, "Save is visible");
            input.validate().unwrap();
            assert!(matches!(&input.questions["save"], decision::Question::Noul { instructions, .. } if instructions == "Is Save visible?"));
            Ok(Output::Decision(decision::Response {
                answers: BTreeMap::from([("save".into(), decision::Answer::Noul { noul:0.75 })]),
                usage: decision::Usage { input_tokens:12, output_tokens:0 },
            }))
        },
    ).unwrap();
    assert_eq!(
        result,
        json!({"answers":{"save":{"type":"noul","noul":0.75}},"usage":{"input_tokens":12,"output_tokens":0}})
    );
}

#[test]
fn runs_in_read_only_callbacks() {
    let result = run_callback(
        bundle(r#"
            import {decide} from 'sailry/decisions';
            export async function run() {
                return decide({state:'Page',questions:{action:{type:'choice',instructions:'Choose',criteria:{save:null,cancel:'Discard'}}}});
            }
        "#),
        "run", json!(null),
        Environment {context:context(),target:NodeId([1;32]),stop:CancellationToken::new(),deadline:Instant::now()+CALL_TIMEOUT},
        true,
        |request| {
            assert!(!request.command.durable());
            assert!(matches!(request.command,Command::Decide(_)));
            Err(Fault::new(ErrorCode::Unavailable,"decision model is not selected"))
        },
    ).unwrap_err();
    assert_eq!(result.code, ErrorCode::Unavailable);
}
