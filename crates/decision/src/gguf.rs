//! Bounded GGUF v3 validation before passing a file to native code.
use crate::{Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

const MAX_ENTRIES: u64 = 262_144;
const MAX_METADATA: u64 = 64 * 1024 * 1024;
const MAX_STRING: u64 = 16 * 1024 * 1024;
const MAX_ARRAY: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    Laya,
    OpenJev,
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub architecture: String,
    pub algorithm: Algorithm,
    pub context: usize,
    pub(crate) template: String,
    pub(crate) head: usize,
    pub(crate) temperatures: BTreeMap<String, f32>,
    pub(crate) bias: BTreeMap<usize, Vec<f32>>,
}

enum Value {
    Unsigned(u64),
    Signed(i64),
    Float(f64),
    String(String),
    Other,
}

impl Value {
    fn unsigned(&self) -> Option<u64> {
        match self {
            Self::Unsigned(value) => Some(*value),
            Self::Signed(value) => u64::try_from(*value).ok(),
            _ => None,
        }
    }
    fn float(&self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(*value as f32),
            Self::Signed(value) => Some(*value as f32),
            Self::Unsigned(value) => Some(*value as f32),
            _ => None,
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
}

struct Reader {
    file: BufReader<File>,
    length: u64,
    position: u64,
}

impl Reader {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self
            .position
            .checked_add(N as u64)
            .filter(|end| *end <= self.length)
            .ok_or(Error::Invalid("truncated GGUF data"))?;
        let mut bytes = [0; N];
        self.file.read_exact(&mut bytes)?;
        self.position = end;
        Ok(bytes)
    }
    fn skip(&mut self, count: u64) -> Result<()> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.length)
            .ok_or(Error::Invalid("truncated GGUF data"))?;
        self.file.seek(SeekFrom::Start(end))?;
        self.position = end;
        Ok(())
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self, keep: bool, limit: u64) -> Result<Option<String>> {
        let length = self.u64()?;
        if length > limit {
            return Err(Error::Invalid("GGUF string exceeds its limit"));
        }
        if !keep {
            self.skip(length)?;
            return Ok(None);
        }
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.length)
            .ok_or(Error::Invalid("truncated GGUF string"))?;
        let mut bytes = vec![0; length as usize];
        self.file.read_exact(&mut bytes)?;
        self.position = end;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::Invalid("GGUF metadata is not UTF-8"))
    }
    fn value(&mut self, kind: u32, keep: bool) -> Result<Value> {
        let value = match kind {
            0 => Value::Unsigned(u8::from_le_bytes(self.bytes()?) as u64),
            1 => Value::Signed(i8::from_le_bytes(self.bytes()?) as i64),
            2 => Value::Unsigned(u16::from_le_bytes(self.bytes()?) as u64),
            3 => Value::Signed(i16::from_le_bytes(self.bytes()?) as i64),
            4 => Value::Unsigned(self.u32()? as u64),
            5 => Value::Signed(i32::from_le_bytes(self.bytes()?) as i64),
            6 => Value::Float(f32::from_le_bytes(self.bytes()?) as f64),
            7 => {
                if self.bytes::<1>()?[0] > 1 {
                    return Err(Error::Invalid("invalid GGUF boolean"));
                }
                Value::Other
            }
            8 => self
                .string(keep, MAX_STRING)?
                .map(Value::String)
                .unwrap_or(Value::Other),
            9 => {
                let element = self.u32()?;
                let count = self.u64()?;
                if count > MAX_ARRAY || element == 9 || element > 12 {
                    return Err(Error::Invalid("invalid GGUF array"));
                }
                for _ in 0..count {
                    self.value(element, false)?;
                    if self.position > MAX_METADATA {
                        return Err(Error::Invalid("GGUF metadata exceeds its limit"));
                    }
                }
                Value::Other
            }
            10 => Value::Unsigned(self.u64()?),
            11 => Value::Signed(i64::from_le_bytes(self.bytes()?)),
            12 => Value::Float(f64::from_le_bytes(self.bytes()?)),
            _ => return Err(Error::Invalid("unknown GGUF metadata type")),
        };
        Ok(value)
    }
}

/// Validates one ordinary, unsplit model without initializing the native runtime.
pub fn validate(path: &Path) -> Result<Metadata> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_file() || metadata.len() > sailry_protocol::models::MAX_ASSET_BYTES {
        return Err(Error::Invalid("GGUF must be a bounded ordinary file"));
    }
    let mut reader = Reader {
        file: BufReader::new(File::open(path)?),
        length: metadata.len(),
        position: 0,
    };
    if reader.bytes::<4>()? != *b"GGUF" || reader.u32()? != 3 {
        return Err(Error::Invalid("GGUF version must be 3"));
    }
    let tensors = reader.u64()?;
    let count = reader.u64()?;
    if tensors == 0 || tensors > MAX_ENTRIES || count > MAX_ENTRIES {
        return Err(Error::Invalid("GGUF entry count exceeds its limit"));
    }
    let mut values = BTreeMap::new();
    let mut keys = BTreeSet::new();
    for _ in 0..count {
        let key = reader.string(true, 4096)?.expect("retained key");
        if key.is_empty() || !keys.insert(key.clone()) {
            return Err(Error::Invalid("invalid or duplicate GGUF key"));
        }
        let kind = reader.u32()?;
        let keep = key == "general.architecture"
            || key == "general.alignment"
            || key == "split.count"
            || key == "tokenizer.chat_template.systemone"
            || key.ends_with(".context_length")
            || key.contains(".decision.");
        let value = reader.value(kind, keep)?;
        if keep {
            values.insert(key, value);
        }
        if reader.position > MAX_METADATA {
            return Err(Error::Invalid("GGUF metadata exceeds its limit"));
        }
    }
    let alignment = values
        .get("general.alignment")
        .map(Value::unsigned)
        .unwrap_or(Some(32))
        .ok_or(Error::Invalid("invalid GGUF alignment"))?;
    if !alignment.is_power_of_two() || alignment > 4096 {
        return Err(Error::Invalid("invalid GGUF alignment"));
    }
    if values
        .get("split.count")
        .is_some_and(|value| value.unsigned() != Some(1))
    {
        return Err(Error::Invalid("split GGUF models are unsupported"));
    }
    let mut ranges = Vec::with_capacity(tensors as usize);
    let mut names = BTreeSet::new();
    for _ in 0..tensors {
        let name = reader.string(true, 4096)?.expect("retained name");
        if name.is_empty() || !names.insert(name) {
            return Err(Error::Invalid("invalid or duplicate GGUF tensor"));
        }
        let dimensions = reader.u32()?;
        if !(1..=4).contains(&dimensions) {
            return Err(Error::Invalid("invalid GGUF tensor dimensions"));
        }
        let mut elements = 1u64;
        let mut row = 0;
        for index in 0..dimensions {
            let dimension = reader.u64()?;
            if dimension == 0 || dimension > i64::MAX as u64 {
                return Err(Error::Invalid("invalid GGUF tensor dimension"));
            }
            if index == 0 {
                row = dimension;
            }
            elements = elements
                .checked_mul(dimension)
                .ok_or(Error::Invalid("GGUF tensor dimensions overflow"))?;
        }
        let (block, bytes) = tensor_layout(reader.u32()?)?;
        if !row.is_multiple_of(block) {
            return Err(Error::Invalid("GGUF tensor row is not block aligned"));
        }
        let size = (elements / block)
            .checked_mul(bytes)
            .ok_or(Error::Invalid("GGUF tensor size overflows"))?;
        let offset = reader.u64()?;
        if !offset.is_multiple_of(alignment) {
            return Err(Error::Invalid("GGUF tensor offset is not aligned"));
        }
        let end = offset
            .checked_add(size)
            .ok_or(Error::Invalid("GGUF tensor range overflows"))?;
        ranges.push((offset, end));
        if reader.position > MAX_METADATA {
            return Err(Error::Invalid("GGUF tensor table exceeds its limit"));
        }
    }
    let start = reader
        .position
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .filter(|value| *value <= reader.length)
        .ok_or(Error::Invalid("truncated GGUF tensor data"))?;
    ranges.sort_unstable();
    let mut previous = 0;
    for (offset, end) in ranges {
        if offset < previous || end > reader.length - start {
            return Err(Error::Invalid("invalid GGUF tensor range"));
        }
        previous = end;
    }
    decision(values)
}

fn decision(values: BTreeMap<String, Value>) -> Result<Metadata> {
    let architecture = values
        .get("general.architecture")
        .and_then(Value::text)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .ok_or(Error::Invalid("GGUF architecture is missing"))?
        .to_owned();
    let prefix = format!("{architecture}.decision.");
    let algorithm = match values.get(&(prefix.clone() + "type")).and_then(Value::text) {
        Some("laya") => Algorithm::Laya,
        Some("openjev") => Algorithm::OpenJev,
        _ => return Err(Error::Invalid("unsupported GGUF decision type")),
    };
    let context = values
        .get(&format!("{architecture}.context_length"))
        .and_then(Value::unsigned)
        .filter(|value| *value > 0 && *value <= 1_048_576)
        .ok_or(Error::Invalid("GGUF context length is missing"))? as usize;
    let template = values
        .get("tokenizer.chat_template.systemone")
        .and_then(Value::text)
        .filter(|value| !value.is_empty() && value.len() <= 128 * 1024 && !value.contains('\0'))
        .ok_or(Error::Invalid("GGUF systemone template is missing"))?
        .to_owned();
    let head = values
        .get(&(prefix.clone() + "max_head_tokens"))
        .and_then(Value::unsigned)
        .unwrap_or(0) as usize;
    if algorithm == Algorithm::Laya && (head == 0 || head > context) {
        return Err(Error::Invalid("GGUF decision head limit is invalid"));
    }
    let mut temperatures = BTreeMap::new();
    let mut bias = BTreeMap::<usize, Vec<f32>>::new();
    for (key, value) in &values {
        if let Some(name) = key.strip_prefix(&(prefix.clone() + "temperature.")) {
            let value = value
                .float()
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or(Error::Invalid("invalid GGUF decision temperature"))?;
            temperatures.insert(name.to_owned(), value);
        } else if let Some(name) = key.strip_prefix(&(prefix.clone() + "score_bias.")) {
            let (levels, index) = name
                .split_once('.')
                .and_then(|(levels, index)| {
                    Some((levels.parse::<usize>().ok()?, index.parse::<usize>().ok()?))
                })
                .filter(|(levels, index)| (2..=255).contains(levels) && index < levels)
                .ok_or(Error::Invalid("invalid GGUF decision score bias"))?;
            let value = value
                .float()
                .filter(|value| value.is_finite())
                .ok_or(Error::Invalid("invalid GGUF decision score bias"))?;
            bias.entry(levels).or_insert_with(|| vec![0.0; levels])[index] = value;
        }
    }
    Ok(Metadata {
        architecture,
        algorithm,
        context,
        template,
        head,
        temperatures,
        bias,
    })
}

// ggml_type and block layouts from llama.cpp b11756 ggml-common.h / ggml.c.
fn tensor_layout(kind: u32) -> Result<(u64, u64)> {
    Ok(match kind {
        0 | 26 => (1, 4),
        1 | 25 | 30 => (1, 2),
        2 | 20 => (32, 18),
        3 => (32, 20),
        6 => (32, 22),
        7 => (32, 24),
        8 => (32, 34),
        9 => (32, 36),
        10 => (256, 84),
        11 | 21 => (256, 110),
        12 => (256, 144),
        13 => (256, 176),
        14 => (256, 210),
        15 => (256, 292),
        16 | 35 => (256, 66),
        17 => (256, 74),
        18 => (256, 98),
        19 => (256, 50),
        22 => (256, 82),
        23 => (256, 136),
        24 => (1, 1),
        27 | 28 => (1, 8),
        29 => (256, 56),
        34 => (256, 54),
        39 => (32, 17),
        40 => (64, 36),
        41 => (128, 18),
        42 => (64, 18),
        _ => return Err(Error::Invalid("unsupported GGUF tensor type")),
    })
}

#[cfg(test)]
mod tests;
