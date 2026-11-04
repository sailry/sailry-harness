use super::*;

fn numbered(lines: usize) -> String {
    (1..=lines).map(|line| format!("line {line}\n")).collect()
}

fn range(text: &str, needle: &str) -> Range<usize> {
    let start = text.find(needle).unwrap();
    start..start + needle.len()
}

fn numbers(hunk: &FileHunk) -> Vec<usize> {
    hunk.lines.iter().map(|line| line.number).collect()
}

#[test]
fn shows_context_around_each_change() {
    let text = numbered(30);
    let excerpt = collect(
        &text,
        &[range(&text, "line 10\n"), range(&text, "line 25\n")],
    );
    assert_eq!(excerpt.hunks.len(), 2);
    assert_eq!(numbers(&excerpt.hunks[0]), (7..=13).collect::<Vec<_>>());
    assert_eq!(numbers(&excerpt.hunks[1]), (22..=28).collect::<Vec<_>>());
    assert_eq!(excerpt.hunks[0].lines[3].text, "line 10\n");
    assert_eq!(excerpt.omitted, 0);
    assert!(!excerpt.hunks[0].shortened);
}

#[test]
fn merges_nearby_changes_and_clamps_edges() {
    let text = numbered(8);
    let excerpt = collect(&text, &[range(&text, "line 1\n"), range(&text, "line 5\n")]);
    assert_eq!(excerpt.hunks.len(), 1);
    assert_eq!(numbers(&excerpt.hunks[0]), (1..=8).collect::<Vec<_>>());
}

#[test]
fn locates_deletions_and_missing_final_newline() {
    let text = "first\nsecond\nlast";
    let excerpt = collect(text, std::slice::from_ref(&(text.len()..text.len())));
    assert_eq!(numbers(&excerpt.hunks[0]), vec![1, 2, 3]);
    assert_eq!(excerpt.hunks[0].lines[2].text, "last");
    assert!(collect("", std::slice::from_ref(&(0..0))).hunks.is_empty());
}

#[test]
fn bounds_hunks_lines_and_bytes() {
    let text = numbered(2000);
    let changes: Vec<_> = (0..40)
        .map(|index| range(&text, &format!("line {}\n", index * 40 + 20)))
        .collect();
    let excerpt = collect(&text, &changes);
    assert_eq!(excerpt.hunks.len(), MAX_HUNKS);
    assert_eq!(excerpt.omitted, 40 - MAX_HUNKS);
    let long = numbered(200);
    let excerpt = collect(&long, std::slice::from_ref(&(0..long.len())));
    assert_eq!(excerpt.hunks[0].lines.len(), HUNK_LINES);
    assert!(excerpt.hunks[0].shortened);
    let wide = format!("{}\n", "中".repeat(LINE_BYTES));
    let excerpt = collect(&wide, std::slice::from_ref(&(0..wide.len())));
    assert!(excerpt.hunks[0].shortened);
    assert!(excerpt.hunks[0].lines[0].text.len() <= LINE_BYTES);
    let heavy = format!("{}\n", "x".repeat(900)).repeat(100);
    let excerpt = collect(&heavy, std::slice::from_ref(&(0..heavy.len())));
    let bytes: usize = excerpt.hunks[0]
        .lines
        .iter()
        .map(|line| line.text.len())
        .sum();
    assert!(bytes <= TOTAL_BYTES);
    assert!(excerpt.hunks[0].shortened);
    assert!(collect(&text, &[]).hunks.is_empty());
}
