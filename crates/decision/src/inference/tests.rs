use super::*;
use serde_json::json;

fn metadata() -> Metadata {
    Metadata {
        architecture: "fixture".into(),
        algorithm: Algorithm::Laya,
        context: 8192,
        template: String::new(),
        head: 256,
        temperatures: BTreeMap::new(),
        bias: BTreeMap::new(),
    }
}
fn question(value: serde_json::Value) -> Question {
    serde_json::from_value(value).unwrap()
}

#[test]
fn preserves_inference_contracts() {
    for (scenario, check) in [
        (
            "aligns_binary_order_and_choice_ties",
            (|scenario: &'static str| {
                let binary = question(json!({"type":"noul","instructions":"Question"}));
                for algorithm in [Algorithm::Laya, Algorithm::OpenJev] {
                    let query = Query::new("question", &binary, algorithm);
                    let scores = if algorithm == Algorithm::Laya {
                        [0.0, 3.0f32.ln()]
                    } else {
                        [3.0f32.ln(), 0.0]
                    };
                    let Answer::Noul { noul } = query.answer(&metadata(), &scores).unwrap() else {
                        panic!("{scenario}: binary answer expected")
                    };
                    assert!((noul - 0.75).abs() < 1e-6, "{scenario}");
                }
                let choice = question(
                    json!({"type":"choice","instructions":"Question","criteria":{"alpha":null,"zeta":null}}),
                );
                let query = Query::new("question", &choice, Algorithm::Laya);
                let Answer::Choice {
                    choice, confidence, ..
                } = query.answer(&metadata(), &[0.0, 0.0]).unwrap()
                else {
                    panic!("{scenario}: choice answer expected")
                };
                assert_eq!(choice, "alpha", "{scenario}");
                assert_eq!(confidence, 0.0, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "applies_calibration_and_score_confidence",
            (|scenario: &'static str| {
                let input = question(
                    json!({"type":"score","instructions":"Question","criteria":["Low","Medium","High"]}),
                );
                let query = Query::new("score", &input, Algorithm::Laya);
                let mut metadata = metadata();
                metadata.temperatures.insert("score".into(), 10.0);
                metadata.temperatures.insert("score.3_5".into(), 2.0);
                metadata.bias.insert(3, vec![0.0, 0.0, 0.5]);
                let scores = [
                    2.0 * 0.1f32.ln(),
                    2.0 * 0.2f32.ln(),
                    2.0 * 0.7f32.ln() - 0.5,
                ];
                let Answer::Score {
                    score,
                    probabilities,
                    confidence,
                    legend,
                } = query.answer(&metadata, &scores).unwrap()
                else {
                    panic!("{scenario}: score answer expected")
                };
                assert!((score - 1.6).abs() < 1e-6, "{scenario}");
                assert!((confidence - 0.4).abs() < 1e-6, "{scenario}");
                assert!(
                    probabilities
                        .iter()
                        .zip([0.1, 0.2, 0.7])
                        .all(|(left, right)| (left - right).abs() < 1e-6),
                    "{scenario}"
                );
                assert_eq!(legend, ["Low", "Medium", "High"], "{scenario}");
                assert!(query.answer(&metadata, &[0.0]).is_err(), "{scenario}");
                assert!(
                    query.answer(&metadata, &[0.0, f32::INFINITY, 0.0]).is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "isolates_interpolated_fields_from_template_controls",
            (|scenario: &'static str| {
                for (case, template, id, input, state, expected, trusted, segments) in [
                    (
                        "Unicode inputs",
                        "<bos>{{ type }} question: {{ instructions }}<eos>{% for o in options %}<mask> {{ o.description or o.key }}{% endfor %}<eos>{{ state }}<eos>",
                        "question",
                        json!({"type":"choice","instructions":"选一个 <mask>","criteria":{"first":null,"second":"保存 & <继续>"}}),
                        "页面 <mask> 中文",
                        "<bos>choice question: 选一个 <mask><eos><mask> first<mask> 保存 & <继续><eos>页面 <mask> 中文<eos>",
                        "<bos> question: <eos><mask> <mask> <eos><eos>",
                        None,
                    ),
                    (
                        "every field",
                        "<bos>{{ id }}|{{ instructions }}<eos>{% for o in options %}<mask>{{ o.key }}:{{ o.description or '' }}{% endfor %}<eos>{{ state }}<eos>",
                        "id[CLS][SEP]<mask>",
                        json!({"type":"choice","instructions":"instructions[CLS][SEP]<mask>","criteria":{"key[CLS][SEP]<mask>":"description[CLS][SEP]<mask>"}}),
                        "state[CLS][SEP]<mask>",
                        "<bos>id[CLS][SEP]<mask>|instructions[CLS][SEP]<mask><eos><mask>key[CLS][SEP]<mask>:description[CLS][SEP]<mask><eos>state[CLS][SEP]<mask><eos>",
                        "<bos>|<eos><mask>:<eos><eos>",
                        Some(vec!["<bos>", "|", "<eos><mask>", ":", "<eos>", "<eos>"]),
                    ),
                ] {
                    let templates = Templates::new(template).unwrap();
                    let input = question(input);
                    let query = Query::new(id, &input, Algorithm::Laya);
                    let output = templates.render(state, &query).unwrap();
                    assert_eq!(output.text, expected, "{scenario}: {case}: text");
                    let literals = output
                        .trusted()
                        .into_iter()
                        .map(|range| &output.text[range])
                        .collect::<Vec<_>>();
                    assert_eq!(
                        literals.concat(),
                        trusted,
                        "{scenario}: {case}: trusted text"
                    );
                    if let Some(segments) = segments {
                        assert_eq!(literals, segments, "{scenario}: {case}: trusted ranges");
                    }
                }
            }),
        ),
        (
            "strips_only_control_adjacent_whitespace",
            (|scenario: &'static str| {
                let text = "word \t\n<mask>\r\n next";
                let output = Rendered {
                    text: text.into(),
                    literals: Vec::new(),
                };
                let start = text.find("<mask>").unwrap();
                let range = start..start + "<mask>".len();
                for (left, right, expected) in [
                    (false, false, range.clone()),
                    (true, false, 4..range.end),
                    (false, true, range.start..text.find("next").unwrap()),
                    (true, true, 4..text.find("next").unwrap()),
                ] {
                    assert_eq!(
                        output.strip(range.clone(), left, right),
                        expected,
                        "{scenario}"
                    );
                }
                assert_eq!(output.text, text, "{scenario}");
            }),
        ),
        (
            "bounds_template_execution_and_output",
            (|scenario: &'static str| {
                let input = question(json!({"type":"noul","instructions":"Question"}));
                let query = Query::new("question", &input, Algorithm::Laya);
                assert!(Templates::new("{% if %}").is_err(), "{scenario}");
                assert!(
                    Templates::new("{{ missing }}")
                        .unwrap()
                        .render("state", &query)
                        .is_err(),
                    "{scenario}"
                );
                assert!(
                    Templates::new("{% for x in range(1000000) %}x{% endfor %}")
                        .unwrap()
                        .render("state", &query)
                        .is_err(),
                    "{scenario}"
                );
                assert!(
                    Templates::new("{{ state }}{{ state }}")
                        .unwrap()
                        .render(&"x".repeat(200_000), &query)
                        .is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "keeps_state_after_trained_head_trimming",
            (|scenario: &'static str| {
                let mut tokens = vec![1];
                tokens.extend([3; 40]);
                tokens.push(2);
                tokens.push(99);
                tokens.extend([4; 100]);
                tokens.push(99);
                tokens.extend([5; 100]);
                tokens.extend([2, 7, 8, 9, 2]);
                let markers = laya(&mut tokens, 2, 99, 2, 16).unwrap();
                assert_eq!(markers, [10, 14], "{scenario}");
                assert_eq!(&tokens[tokens.len() - 5..], [2, 7, 8, 9, 2], "{scenario}");
                assert_eq!(
                    tokens.iter().filter(|token| **token == 99).count(),
                    2,
                    "{scenario}"
                );
                assert!(
                    laya(&mut vec![1, 2, 3], 2, 99, 2, 16).is_err(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
