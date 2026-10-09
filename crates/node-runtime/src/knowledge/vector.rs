//! Compact derived vectors; source text and model identity remain authoritative.
use sailry_protocol::Fault;

#[derive(Clone, Debug)]
pub(super) struct Vector {
    values: Vec<i8>,
    scale: f32,
    norm: f32,
}

impl Vector {
    pub(super) fn quantize(values: &[f32]) -> Result<Self, Fault> {
        if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
            return Err(Fault::invalid("invalid knowledge embedding"));
        }
        let peak = values.iter().copied().map(f32::abs).fold(0., f32::max);
        if peak == 0. {
            return Err(Fault::invalid("knowledge embedding has no direction"));
        }
        let scale = peak / 127.;
        if scale == 0. {
            return Err(Fault::invalid(
                "knowledge embedding is too small to quantize",
            ));
        }
        let values = values
            .iter()
            .map(|value| (value / scale).round().clamp(-127., 127.) as i8)
            .collect::<Vec<_>>();
        let norm = values
            .iter()
            .map(|value| f32::from(*value).powi(2))
            .sum::<f32>()
            .sqrt();
        Ok(Self {
            values,
            scale,
            norm,
        })
    }

    pub(super) fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + self.values.len());
        bytes.extend_from_slice(&self.scale.to_le_bytes());
        bytes.extend(self.values.iter().map(|value| *value as u8));
        bytes
    }

    pub(super) fn decode(bytes: &[u8], dimension: usize) -> Result<Self, Fault> {
        if dimension == 0 || bytes.len() != dimension + 4 {
            return Err(Fault::invalid("invalid knowledge vector dimension"));
        }
        let scale = f32::from_le_bytes(bytes[..4].try_into().unwrap());
        if !scale.is_finite() || scale <= 0. {
            return Err(Fault::invalid("invalid knowledge vector scale"));
        }
        let values = bytes[4..]
            .iter()
            .map(|value| *value as i8)
            .collect::<Vec<_>>();
        if values.contains(&i8::MIN) {
            return Err(Fault::invalid("invalid knowledge vector value"));
        }
        let norm = values
            .iter()
            .map(|value| f32::from(*value).powi(2))
            .sum::<f32>()
            .sqrt();
        if norm == 0. {
            return Err(Fault::invalid("knowledge vector has no direction"));
        }
        Ok(Self {
            values,
            scale,
            norm,
        })
    }

    pub(super) fn cosine(&self, query: &[f32]) -> Result<f32, Fault> {
        if query.len() != self.values.len() || query.iter().any(|value| !value.is_finite()) {
            return Err(Fault::invalid(
                "knowledge query dimension differs from its index",
            ));
        }
        let norm = query.iter().map(|value| value.powi(2)).sum::<f32>().sqrt();
        if norm == 0. || !norm.is_finite() {
            return Err(Fault::invalid("knowledge query embedding has no direction"));
        }
        let dot = self
            .values
            .iter()
            .zip(query)
            .map(|(value, query)| f32::from(*value) * query)
            .sum::<f32>();
        Ok((dot / (self.norm * norm)).clamp(-1., 1.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_direction_and_round_trips() {
        let values = [0.1, -0.6, 0.2, 0.75, 0.];
        let vector = Vector::quantize(&values).unwrap();
        let bytes = vector.encode();
        assert_eq!(bytes.len(), values.len() + 4);
        let restored = Vector::decode(&bytes, values.len()).unwrap();
        assert!(restored.cosine(&values).unwrap() > 0.9999);
        assert_eq!(restored.encode(), bytes);
        let opposite = values.map(|value| -value);
        assert!(restored.cosine(&opposite).unwrap() < -0.9999);
    }

    #[test]
    fn rejects_corrupt_or_incompatible_vectors() {
        for values in [
            vec![],
            vec![0., 0.],
            vec![f32::NAN],
            vec![f32::INFINITY],
            vec![f32::from_bits(1)],
        ] {
            assert!(Vector::quantize(&values).is_err());
        }
        let mut bytes = Vector::quantize(&[1., 0.]).unwrap().encode();
        assert!(Vector::decode(&bytes, 3).is_err());
        bytes[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Vector::decode(&bytes, 2).is_err());
        assert!(Vector::quantize(&[1., 0.]).unwrap().cosine(&[1.]).is_err());
    }
}
