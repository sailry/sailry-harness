//! Derived from Goose 14741e9a, goose-context-management/src/templates.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use minijinja::{Environment, Error, syntax::SyntaxConfig, value::Serde};
use serde::Serialize;

pub(super) fn render(template: &str, context: &impl Serialize) -> Result<String, Error> {
    let mut environment = Environment::new();
    environment.set_syntax(
        SyntaxConfig::builder()
            .trim_blocks(true)
            .lstrip_blocks(true)
            .build()?,
    );
    environment.add_filter("code_fence", code_fence);
    environment.add_template("template", template)?;
    let rendered = environment
        .get_template("template")?
        .render(Serde(context))?;
    Ok(rendered.trim().to_owned())
}

fn code_fence(code: String) -> String {
    let longest = code
        .chars()
        .fold((0usize, 0usize), |(longest, run), character| {
            if character == '`' {
                (longest.max(run + 1), run + 1)
            } else {
                (longest, 0)
            }
        })
        .0;
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}\n{}\n{fence}", code.trim_end_matches('\n'))
}
