use crate::Error;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) struct Table {
    file: File,
    offset: u64,
    rows: usize,
    dimension: usize,
    unknown: Option<u32>,
}

#[derive(Deserialize)]
struct Header {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: [u64; 2],
}

impl Table {
    pub fn load(path: &Path, unknown: Option<u32>) -> Result<Self, Error> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut size = [0u8; 8];
        file.read_exact(&mut size)?;
        let size = u64::from_le_bytes(size);
        if size == 0 || size > 1024 * 1024 {
            return Err(Error::Invalid(
                "static tensor header exceeds the size limit".into(),
            ));
        }
        let mut bytes = vec![0u8; size as usize];
        file.read_exact(&mut bytes)?;
        let mut headers: BTreeMap<String, serde_json::Value> = serde_json::from_slice(&bytes)?;
        headers.remove("__metadata__");
        if headers.len() != 1 {
            return Err(Error::Invalid(
                "static model must contain one embedding tensor".into(),
            ));
        }
        let header: Header =
            serde_json::from_value(headers.remove("embeddings").ok_or_else(|| {
                Error::Invalid("static model is missing its embedding tensor".into())
            })?)?;
        let [rows, dimension] = header.shape.as_slice() else {
            return Err(Error::Invalid(
                "static embedding tensor must have two dimensions".into(),
            ));
        };
        let bytes = rows
            .checked_mul(*dimension)
            .and_then(|size| size.checked_mul(4));
        let offset = 8u64
            .checked_add(size)
            .and_then(|size| size.checked_add(header.data_offsets[0]));
        if header.dtype != "F32"
            || *rows == 0
            || *dimension == 0
            || header.data_offsets[0] != 0
            || bytes.and_then(|size| u64::try_from(size).ok()) != Some(header.data_offsets[1])
            || offset.and_then(|offset| offset.checked_add(header.data_offsets[1])) != Some(length)
            || unknown.is_some_and(|id| id as usize >= *rows)
        {
            return Err(Error::Invalid(
                "static embedding tensor layout is invalid".into(),
            ));
        }
        Ok(Self {
            file,
            offset: offset.expect("validated static data offset"),
            rows: *rows,
            dimension: *dimension,
            unknown,
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn mean(&mut self, ids: &[u32]) -> Result<Vec<f32>, Error> {
        let mut sums = vec![0.0f64; self.dimension];
        let mut count = 0usize;
        let mut bytes = vec![0u8; self.dimension * 4];
        for &id in ids {
            if Some(id) == self.unknown {
                continue;
            }
            if id as usize >= self.rows {
                return Err(Error::Invalid("token ID exceeds the static table".into()));
            }
            self.file.seek(SeekFrom::Start(
                self.offset + u64::from(id) * bytes.len() as u64,
            ))?;
            self.file.read_exact(&mut bytes)?;
            for (sum, bytes) in sums.iter_mut().zip(bytes.as_chunks::<4>().0) {
                let value = f32::from_le_bytes(*bytes);
                if !value.is_finite() {
                    return Err(Error::Invalid(
                        "static table contains a nonfinite value".into(),
                    ));
                }
                *sum += f64::from(value);
            }
            count += 1;
        }
        Ok(sums
            .into_iter()
            .map(|sum| {
                if count == 0 {
                    0.0
                } else {
                    (sum / count as f64) as f32
                }
            })
            .collect())
    }
}
