use std::ops::Range;

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Default)]
pub(crate) struct Encoding {
    #[cfg(windows)]
    windows: Option<windows::CodePage>,
    #[cfg(windows)]
    command: bool,
}

impl Encoding {
    #[cfg(all(test, windows))]
    pub(super) fn code_page(number: u32) -> Self {
        Self {
            windows: windows::CodePage::new(number),
            command: true,
        }
    }

    pub(crate) fn command() -> Self {
        #[cfg(windows)]
        return Self {
            windows: windows::CodePage::current(),
            command: true,
        };
        #[cfg(not(windows))]
        Self::default()
    }
}

#[derive(Default)]
pub(super) struct Decoder {
    #[cfg(windows)]
    windows: Option<windows::Decoder>,
}

impl Decoder {
    pub(super) fn new(_encoding: Encoding, _prefix: usize, _tail: usize) -> Self {
        Self {
            #[cfg(windows)]
            windows: _encoding
                .command
                .then(|| windows::Decoder::new(_encoding.windows, _prefix, _tail)),
        }
    }

    pub(super) fn append(&mut self, _bytes: &[u8]) {
        #[cfg(windows)]
        if let Some(decoder) = &mut self.windows {
            decoder.append(_bytes);
        }
    }

    pub(super) fn finish(&mut self) {
        #[cfg(windows)]
        if let Some(decoder) = &mut self.windows {
            decoder.finish();
        }
    }

    pub(super) fn native(&self) -> bool {
        #[cfg(windows)]
        if let Some(decoder) = &self.windows {
            return decoder.native();
        }
        false
    }

    pub(super) fn transcoded(&self) -> bool {
        #[cfg(windows)]
        if let Some(decoder) = &self.windows {
            return decoder.transcoded();
        }
        false
    }

    pub(super) fn split(&self, _offset: usize) -> bool {
        #[cfg(windows)]
        if let Some(decoder) = &self.windows {
            return decoder.split(_offset);
        }
        false
    }

    pub(super) fn bounds(&self, bytes: &[u8], offset: usize, complete: bool) -> Range<usize> {
        #[cfg(windows)]
        if let Some(decoder) = &self.windows
            && let Some(range) = decoder.bounds(bytes, offset, complete)
        {
            return range;
        }
        let start = if offset > 0 {
            super::edges(bytes, 0, bytes.len()).1
        } else {
            0
        };
        let end = if complete {
            bytes.len()
        } else {
            super::edges(bytes, bytes.len(), 0).0
        };
        start.min(end)..end
    }

    pub(super) fn page(
        &self,
        bytes: &[u8],
        offset: usize,
        limit: usize,
        _complete: bool,
    ) -> Result<Range<usize>, sailry_protocol::Fault> {
        #[cfg(windows)]
        if let Some(decoder) = &self.windows {
            return decoder.page(bytes, offset, limit, _complete);
        }
        if offset > 0 && bytes.first().is_some_and(|byte| byte & 0xc0 == 0x80) {
            return Err(sailry_protocol::Fault::invalid(
                "command output offset splits a UTF-8 character",
            ));
        }
        super::utf8_page(bytes, 0, limit)
    }

    pub(super) fn decode(&self, bytes: &[u8], _offset: usize) -> (String, bool) {
        #[cfg(windows)]
        if let Some(decoder) = &self.windows {
            return decoder.decode(bytes, _offset);
        }
        (
            String::from_utf8_lossy(bytes).into_owned(),
            std::str::from_utf8(bytes).is_err(),
        )
    }
}
