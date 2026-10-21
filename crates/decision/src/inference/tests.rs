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
fn aligns_binary_order_and_choice_ties() {
    let binary = question(json!({"type":"noul","instructions":"Question"}));
    for algorithm in [Algorithm::Laya, Algorithm::OpenJev] {
        let query = Query::new("question", &binary, algorithm);
        let scores = if algorithm == Algorithm::Laya {
            [0.0, 3.0f32.ln()]
        } else {
            [3.0f32.ln(), 0.0]
        };
        let Answer::Noul { noul } = query.answer(&metadata(), &scores).unwrap() else {
            panic!("binary answer expected")
        };
        assert!((noul - 0.75).abs() < 1e-6);
    }
    let choice = question(
        json!({"type":"choice","instructions":"Question","criteria":{"alpha":null,"zeta":null}}),
    );
    let query = Query::new("question", &choice, Algorithm::Laya);
    let Answer::Choice {
        choice, confidence, ..
    } = query.answer(&metadata(), &[0.0, 0.0]).unwrap()
    else {
        panic!("choice answer expected")
    };
    assert_eq!(choice, "alpha");
    assert_eq!(confidence, 0.0);
}

#[test]
fn applies_calibration_and_score_confidence() {
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
        panic!("score answer expected")
    };
    assert!((score - 1.6).abs() < 1e-6);
    assert!((confidence - 0.4).abs() < 1e-6);
    assert!(
        probabilities
            .iter()
            .zip([0.1, 0.2, 0.7])
            .all(|(left, right)| (left - right).abs() < 1e-6)
    );
    assert_eq!(legend, ["Low", "Medium", "High"]);
    assert!(query.answer(&metadata, &[0.0]).is_err());
    assert!(query.answer(&metadata, &[0.0, f32::INFINITY, 0.0]).is_err());
}

#[test]
fn preserves_inputs_and_template_controls() {
    let templates = Templates::new("<bos>{{ type }} question: {{ instructions }}<eos>{% for o in options %}<mask> {{ o.description or o.key }}{% endfor %}<eos>{{ state }}<eos>").unwrap();
    let input = question(
        json!({"type":"choice","instructions":"选一个 <mask>","criteria":{"first":null,"second":"保存 & <继续>"}}),
    );
    let query = Query::new("question", &input, Algorithm::Laya);
    let prompt = templates.render("页面 <mask> 中文", &query).unwrap();
    assert_eq!(
        prompt.text,
        "<bos>choice question: 选一个 <mask><eos><mask> first<mask> 保存 & <继续><eos>页面 <mask> 中文<eos>"
    );
    let trusted: String = prompt
        .trusted()
        .into_iter()
        .map(|range| &prompt.text[range])
        .collect();
    assert_eq!(trusted, "<bos> question: <eos><mask> <mask> <eos><eos>");
}

#[test]
fn isolates_every_interpolated_field() {
    let templates = Templates::new("<bos>{{ id }}|{{ instructions }}<eos>{% for o in options %}<mask>{{ o.key }}:{{ o.description or '' }}{% endfor %}<eos>{{ state }}<eos>").unwrap();
    let input = question(json!({
        "type":"choice", "instructions":"instructions[CLS][SEP]<mask>",
        "criteria":{"key[CLS][SEP]<mask>":"description[CLS][SEP]<mask>"}
    }));
    let query = Query::new("id[CLS][SEP]<mask>", &input, Algorithm::Laya);
    let output = templates.render("state[CLS][SEP]<mask>", &query).unwrap();
    assert_eq!(
        output.text,
        "<bos>id[CLS][SEP]<mask>|instructions[CLS][SEP]<mask><eos><mask>key[CLS][SEP]<mask>:description[CLS][SEP]<mask><eos>state[CLS][SEP]<mask><eos>"
    );
    assert_eq!(
        output
            .trusted()
            .into_iter()
            .map(|range| &output.text[range])
            .collect::<Vec<_>>(),
        ["<bos>", "|", "<eos><mask>", ":", "<eos>", "<eos>"]
    );
}

#[test]
fn strips_only_control_adjacent_whitespace() {
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
        assert_eq!(output.strip(range.clone(), left, right), expected);
    }
    assert_eq!(output.text, text);
}

#[test]
fn bounds_template_execution_and_output() {
    let input = question(json!({"type":"noul","instructions":"Question"}));
    let query = Query::new("question", &input, Algorithm::Laya);
    assert!(Templates::new("{% if %}").is_err());
    assert!(
        Templates::new("{{ missing }}")
            .unwrap()
            .render("state", &query)
            .is_err()
    );
    assert!(
        Templates::new("{% for x in range(1000000) %}x{% endfor %}")
            .unwrap()
            .render("state", &query)
            .is_err()
    );
    assert!(
        Templates::new("{{ state }}{{ state }}")
            .unwrap()
            .render(&"x".repeat(200_000), &query)
            .is_err()
    );
}

#[test]
fn keeps_state_after_trained_head_trimming() {
    let mut tokens = vec![1];
    tokens.extend([3; 40]);
    tokens.push(2);
    tokens.push(99);
    tokens.extend([4; 100]);
    tokens.push(99);
    tokens.extend([5; 100]);
    tokens.extend([2, 7, 8, 9, 2]);
    let markers = laya(&mut tokens, 2, 99, 2, 16).unwrap();
    assert_eq!(markers, [10, 14]);
    assert_eq!(&tokens[tokens.len() - 5..], [2, 7, 8, 9, 2]);
    assert_eq!(tokens.iter().filter(|token| **token == 99).count(), 2);
    assert!(laya(&mut vec![1, 2, 3], 2, 99, 2, 16).is_err());
}
