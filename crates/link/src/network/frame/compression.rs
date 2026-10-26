use super::{invalid, length};
use sailry_protocol::Fault;
use zeroize::Zeroizing;

const COMPRESSED: u32 = 1 << 31;
const MIN_BYTES: usize = 1024;

pub(super) fn encode(
    bytes: Zeroizing<Vec<u8>>,
    enabled: bool,
) -> Result<(u32, Zeroizing<Vec<u8>>), Fault> {
    length(bytes.len())?;
    if enabled && bytes.len() >= MIN_BYTES {
        let compressed = Zeroizing::new(zstd::bulk::compress(&bytes, 1).map_err(invalid)?);
        if compressed.len() + 4 < bytes.len() {
            let mut payload = Zeroizing::new(Vec::with_capacity(compressed.len() + 4));
            payload.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            payload.extend_from_slice(&compressed);
            return Ok((COMPRESSED | payload.len() as u32, payload));
        }
    }
    Ok((bytes.len() as u32, bytes))
}

pub(super) fn size(prefix: u32, enabled: bool) -> Result<usize, Fault> {
    if prefix & COMPRESSED != 0 && !enabled {
        return Err(invalid("frame compression was not negotiated"));
    }
    let size = (prefix & !COMPRESSED) as usize;
    length(size)?;
    Ok(size)
}

pub(super) fn decode(
    prefix: u32,
    bytes: &[u8],
    enabled: bool,
    maximum: usize,
) -> Result<Option<Zeroizing<Vec<u8>>>, Fault> {
    if size(prefix, enabled)? != bytes.len() {
        return Err(invalid("frame payload length differs"));
    }
    if bytes.len() > maximum {
        return Err(invalid("frame exceeds protocol limit"));
    }
    if prefix & COMPRESSED == 0 {
        return Ok(None);
    }
    let declared = bytes
        .get(..4)
        .ok_or_else(|| invalid("compressed frame length is missing"))?;
    let decoded = u32::from_be_bytes(declared.try_into().expect("four length bytes")) as usize;
    length(decoded)?;
    if decoded > maximum {
        return Err(invalid("expanded frame exceeds protocol limit"));
    }
    let mut decoder = zstd::bulk::Decompressor::new().map_err(invalid)?;
    decoder
        .window_log_max(sailry_protocol::MAX_FRAME_BYTES.ilog2())
        .map_err(invalid)?;
    // The pinned bulk API returns an error instead of growing past this capacity.
    let decoded_bytes = Zeroizing::new(decoder.decompress(&bytes[4..], decoded).map_err(invalid)?);
    if decoded_bytes.len() != decoded {
        return Err(invalid("expanded frame length differs"));
    }
    Ok(Some(decoded_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::MAX_FRAME_BYTES;

    #[test]
    fn keeps_plain_frames_without_negotiation() {
        let raw = vec![b'x'; 16_384];
        let (prefix, bytes) = encode(Zeroizing::new(raw.clone()), false).unwrap();
        assert_eq!(prefix as usize, raw.len());
        assert_eq!(&*bytes, &raw);
        assert!(decode(prefix, &bytes, false, raw.len()).unwrap().is_none());
    }

    #[test]
    fn rejects_unnegotiated_compression() {
        let raw = vec![b'x'; 16_384];
        let (prefix, bytes) = encode(Zeroizing::new(raw.clone()), true).unwrap();
        assert!(bytes.len() < raw.len());
        assert!(size(prefix, false).is_err());
        assert!(decode(prefix, &bytes, false, raw.len()).is_err());
        assert_eq!(
            &*decode(prefix, &bytes, true, raw.len()).unwrap().unwrap(),
            &raw
        );
    }

    #[test]
    fn bounds_both_frame_lengths() {
        assert!(size(COMPRESSED | (MAX_FRAME_BYTES as u32 + 1), true).is_err());
        let compressed = zstd::bulk::compress(&vec![b'x'; MAX_FRAME_BYTES + 1], 1).unwrap();
        let mut bytes = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes().to_vec();
        bytes.extend(compressed);
        assert!(
            decode(
                COMPRESSED | bytes.len() as u32,
                &bytes,
                true,
                MAX_FRAME_BYTES
            )
            .is_err()
        );
        bytes[..4].copy_from_slice(&1024u32.to_be_bytes());
        assert!(
            decode(
                COMPRESSED | bytes.len() as u32,
                &bytes,
                true,
                MAX_FRAME_BYTES
            )
            .is_err()
        );
        assert!(encode(Zeroizing::new(vec![0; MAX_FRAME_BYTES + 1]), true).is_err());
    }

    #[test]
    fn rejects_malformed_and_mismatched_frames() {
        for bytes in [vec![0, 0, 0], vec![0, 0, 4, 0, 1, 2, 3]] {
            assert!(decode(COMPRESSED | bytes.len() as u32, &bytes, true, 4096).is_err());
        }
        let (prefix, mut bytes) = encode(Zeroizing::new(vec![b'x'; 4096]), true).unwrap();
        bytes[..4].copy_from_slice(&4095u32.to_be_bytes());
        assert!(decode(prefix, &bytes, true, 4096).is_err());
    }
}
