//! Model-only command text; terminal bytes and canonical captures stay untouched.
use std::collections::BTreeMap;
use vte::{Parser, Perform};

#[derive(Default)]
struct Text {
    output: String,
    line: String,
    replace: bool,
}

impl Text {
    fn replace(&mut self) {
        if self.replace {
            self.line.clear();
            self.replace = false;
        }
    }
}

impl Perform for Text {
    fn print(&mut self, character: char) {
        self.replace();
        self.line.push(character);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => self.replace = true,
            b'\n' => {
                self.output.push_str(&self.line);
                self.output.push('\n');
                self.line.clear();
                self.replace = false;
            }
            b'\t' => {
                self.replace();
                self.line.push('\t');
            }
            0x08 => {
                self.line.pop();
            }
            _ => {}
        }
    }
}

pub(crate) fn normalize(text: &str) -> String {
    let mut output = Text::default();
    Parser::new().advance(&mut output, text.as_bytes());
    output.output.push_str(&output.line);
    output.output
}

pub(crate) fn is_cargo(command: &str) -> bool {
    let words: Vec<_> = command.split_whitespace().collect();
    words.windows(2).any(|pair| {
        pair[0]
            .rsplit('/')
            .next()
            .is_some_and(|name| name == "cargo" || name == "cargo.exe")
            && matches!(pair[1], "test" | "nextest" | "clippy" | "build" | "check")
    })
}

/// Remove only unambiguous success/progress rows. Failure blocks stay complete.
pub(crate) fn cargo(command: &str, text: &str) -> Option<String> {
    if !is_cargo(command) {
        return None;
    }
    let mut output = String::new();
    let mut passed = 0;
    let mut compiling = 0;
    let mut warnings = BTreeMap::<String, usize>::new();
    let mut warning = Vec::new();
    let mut failure = false;
    let flush = |warning: &mut Vec<&str>, warnings: &mut BTreeMap<String, usize>| {
        if warning.is_empty() {
            return;
        }
        let rule = warning
            .iter()
            .find_map(|line| {
                let (_, lint) = line.split_once("#[warn(")?;
                Some(lint.split_once(")]")?.0.to_owned())
            })
            .unwrap_or_else(|| warning[0].trim().to_owned());
        *warnings.entry(rule).or_default() += 1;
        warning.clear();
    };
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("failures:")
            || trimmed.starts_with("FAIL [")
            || trimmed.starts_with("error:")
            || trimmed.starts_with("error[")
        {
            failure = true;
        }
        if failure {
            flush(&mut warning, &mut warnings);
            output.push_str(line);
            continue;
        }
        let is_warning = trimmed.starts_with("warning:") && !trimmed.contains(" generated ");
        let continues = !warning.is_empty()
            && !is_warning
            && (trimmed.is_empty()
                || line.starts_with(' ')
                || trimmed.starts_with("-->")
                || trimmed.starts_with('|')
                || trimmed.starts_with('=')
                || trimmed
                    .split_once('|')
                    .is_some_and(|(number, _)| number.trim_end().parse::<usize>().is_ok()));
        if continues {
            warning.push(line);
            continue;
        }
        flush(&mut warning, &mut warnings);
        if is_warning {
            warning.push(line);
        } else if trimmed.starts_with("test ") && trimmed.trim_end().ends_with(" ... ok")
            || trimmed.starts_with("PASS [")
        {
            passed += 1;
        } else if trimmed.starts_with("Compiling ") || trimmed.starts_with("Checking ") {
            compiling += 1;
        } else {
            output.push_str(line);
        }
    }
    flush(&mut warning, &mut warnings);
    if passed + compiling + warnings.values().sum::<usize>() == 0 {
        return None;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&format!(
        "[Omitted {passed} passed tests, {compiling} compilation progress rows"
    ));
    for (rule, count) in warnings {
        output.push_str(&format!("; warning {rule}: {count}"));
    }
    output.push_str("; use read_tool_output with output, offset and limit for complete output]\n");
    (output.len() < text.len()).then_some(output)
}

#[cfg(test)]
mod tests;
