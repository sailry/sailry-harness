//! Model-only command text; terminal bytes and canonical captures stay untouched.
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
