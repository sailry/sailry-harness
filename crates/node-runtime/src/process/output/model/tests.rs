use super::*;

#[test]
fn preserves_failure_blocks() {
    let input = format!(
        "   Compiling sailry v0.1.0\n{}test failed_case ... FAILED\n\nfailures:\n\n---- failed_case stdout ----\nthread 'failed_case' panicked at src/lib.rs:12:4:\nassertion `left == right` failed\n  left: 1\n right: 2\n\nfailures:\n    failed_case\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
        "test successful_case ... ok\n".repeat(100)
    );
    let output = cargo("cargo test --workspace", &input).unwrap();
    assert!(output.contains("---- failed_case stdout ----\nthread 'failed_case' panicked"));
    assert!(output.contains("test result: FAILED."));
    assert!(output.contains("Omitted 100 passed tests, 1 compilation progress rows"));
    assert!(!output.contains("test successful_case"));
}

#[test]
fn keeps_compiler_errors_and_counts_lints() {
    let input = format!(
        "{}warning: unused variable: `value`\n --> src/lib.rs:1:2\n  |\n1 | let value = 1;\n  |     ^^^^^ help: prefix it with an underscore\n  = note: `#[warn(unused_variables)]` on by default\n\nerror[E0308]: mismatched types\n --> src/lib.rs:8:5\n  |\n8 |     true\n  |     ^^^^ expected `u32`, found `bool`\n\nerror: could not compile `sailry` (lib) due to 1 previous error\n",
        "    Checking dependency v1.0.0\n".repeat(40)
    );
    let output = cargo("cargo clippy", &input).unwrap();
    assert!(output.contains("error[E0308]: mismatched types"));
    assert!(output.contains("expected `u32`, found `bool`"));
    assert!(output.contains("warning unused_variables: 1"));
}

#[test]
fn retains_all_passed_summary() {
    let input = format!(
        "{}test result: ok. 100 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        "test ordinary_case ... ok\n".repeat(100)
    );
    let output = cargo("cargo nextest run", &input).unwrap();
    assert!(output.contains("test result: ok. 100 passed"));
}

#[test]
fn folds_nextest_success_rows() {
    let input = format!(
        "{}        FAIL [   0.010s] (101/101) sailry-runtime::fixture rejects_invalid\n\
         ---- STDOUT: sailry-runtime::fixture rejects_invalid ----\n\
         test fixture_output ... ok\n\
         Compiling user-authored output\n\
         Summary [   0.101s] 101 tests run: 100 passed, 1 failed, 0 skipped\n",
        "        PASS [   0.001s] (  1/101) sailry-runtime::fixture preserves_input\n".repeat(100)
    );
    let output = cargo("cargo nextest run", &input).unwrap();
    assert!(!output.contains("PASS ["));
    assert!(output.contains("FAIL [   0.010s]"));
    assert!(output.contains("test fixture_output ... ok\nCompiling user-authored output\n"));
    assert!(output.contains("101 tests run: 100 passed, 1 failed"));
    assert!(output.contains("Omitted 100 passed tests"));
}

#[test]
fn preserves_success_like_failure_output() {
    let input = format!(
        "{}failures:\n\n---- failed_case stdout ----\n\
         test printed_status ... ok\nCompiling printed progress\n\
         warning: printed warning\n\n\
         test result: FAILED. 100 passed; 1 failed; 0 ignored\n",
        "test ordinary_case ... ok\n".repeat(100)
    );
    let output = cargo("cargo test", &input).unwrap();
    assert!(output.contains(
        "test printed_status ... ok\nCompiling printed progress\nwarning: printed warning\n"
    ));
    assert!(output.contains("Omitted 100 passed tests"));
}

#[test]
fn retains_unknown_output() {
    let input = "business output\nunrecognized status\n";
    assert!(cargo("cargo test", input).is_none());
    assert!(cargo("printf cargo", input).is_none());
    assert!(cargo("cargo test", "test single ... ok\n").is_none());
}
