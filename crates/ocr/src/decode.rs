use crate::Error;
use ndarray::ArrayD;
use std::path::Path;

pub(super) struct Dictionary(Vec<String>);

impl Dictionary {
    pub fn load(path: &Path) -> Result<Self, Error> {
        if std::fs::metadata(path)?.len() > 512 * 1024 {
            return Err(Error::Invalid("OCR dictionary is too large".into()));
        }
        Self::parse(&std::fs::read_to_string(path)?)
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let entries = text.lines().map(str::to_owned).collect::<Vec<_>>();
        if entries.is_empty()
            || entries.len() > 65534
            || entries.iter().any(|entry| {
                entry.is_empty() || entry.len() > 64 || entry.chars().any(char::is_control)
            })
        {
            return Err(Error::Invalid("OCR dictionary is invalid".into()));
        }
        let mut tokens = Vec::with_capacity(entries.len() + 2);
        tokens.push(String::new());
        tokens.extend(entries);
        tokens.push(" ".into());
        Ok(Self(tokens))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn decode(&self, output: &ArrayD<f32>) -> Result<(String, f32), Error> {
        let shape = output.shape();
        if shape.len() != 3 || shape[0] != 1 || shape[1] == 0 || shape[2] != self.len() {
            return Err(Error::Invalid(
                "recognizer vocabulary does not match its dictionary".into(),
            ));
        }
        let mut text = String::new();
        let mut total = 0.0;
        let mut count = 0;
        let mut previous = None;
        for row in output
            .outer_iter()
            .next()
            .expect("validated batch")
            .outer_iter()
        {
            if row
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.001).contains(value))
            {
                return Err(Error::Invalid(
                    "recognizer probabilities are invalid".into(),
                ));
            }
            let (index, score) = row
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| left.total_cmp(right))
                .expect("validated vocabulary");
            if index != 0 && previous != Some(index) {
                text.push_str(&self.0[index]);
                total += score;
                count += 1;
            }
            previous = Some(index);
        }
        Ok((
            text,
            if count == 0 {
                0.0
            } else {
                total / count as f32
            },
        ))
    }
}

#[cfg(test)]
mod tests;
