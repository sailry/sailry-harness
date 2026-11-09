use std::ops::Range;
use windows_sys::Win32::{
    Globalization::{CPINFO, GetCPInfo, GetOEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar},
    System::Console::GetConsoleOutputCP,
};

#[derive(Clone, Copy)]
pub(super) struct CodePage {
    number: u32,
    leads: [u8; 12],
    single_byte: bool,
}

impl CodePage {
    pub(super) fn current() -> Option<Self> {
        // SAFETY: these queries have no pointers and do not change console state.
        let console = unsafe { GetConsoleOutputCP() };
        let number = if console == 0 {
            unsafe { GetOEMCP() }
        } else {
            console
        };
        Self::new(number)
    }

    pub(super) fn new(number: u32) -> Option<Self> {
        if number == 65001 {
            return None;
        }
        let mut info = CPINFO::default();
        // SAFETY: info is a writable CPINFO with its exact native layout.
        if unsafe { GetCPInfo(number, &mut info) } == 0 || !matches!(info.MaxCharSize, 1 | 2) {
            return None;
        }
        Some(Self {
            number,
            leads: info.LeadByte,
            single_byte: info.MaxCharSize == 1,
        })
    }

    fn lead(self, byte: u8) -> bool {
        self.leads
            .as_chunks::<2>()
            .0
            .iter()
            .take_while(|range| range[0] != 0)
            .any(|range| (range[0]..=range[1]).contains(&byte))
    }

    fn decode(self, bytes: &[u8]) -> Option<String> {
        if bytes.is_empty() {
            return Some(String::new());
        }
        let count = i32::try_from(bytes.len()).ok()?;
        // SAFETY: the first call only measures the output for this bounded input.
        let length = unsafe {
            MultiByteToWideChar(
                self.number,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                count,
                std::ptr::null_mut(),
                0,
            )
        };
        if length <= 0 {
            return None;
        }
        let mut wide = vec![0; length as usize];
        // SAFETY: both slices remain live and the measured UTF-16 capacity is exact.
        let written = unsafe {
            MultiByteToWideChar(
                self.number,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                count,
                wide.as_mut_ptr(),
                length,
            )
        };
        if written != length {
            return None;
        }
        String::from_utf16(&wide).ok()
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Detect,
    Utf8 { bom: bool },
    Legacy,
    Utf16 { big_endian: bool },
}

pub(super) struct Decoder {
    code_page: Option<CodePage>,
    mode: Mode,
    probe: Vec<u8>,
    total: usize,
    lead: bool,
    unit: Option<u8>,
    surrogate: bool,
    utf8: bool,
    prefix: usize,
    tail: usize,
    trails: Vec<u8>,
}

impl Decoder {
    pub(super) fn new(code_page: Option<CodePage>, prefix: usize, tail: usize) -> Self {
        Self {
            code_page,
            mode: Mode::Detect,
            probe: Vec::new(),
            total: 0,
            lead: false,
            unit: None,
            surrogate: false,
            utf8: false,
            prefix,
            tail,
            trails: Vec::new(),
        }
    }

    fn slot(&self, offset: usize) -> Option<usize> {
        if offset <= self.prefix {
            Some(offset)
        } else if self.tail > 0 {
            Some(self.prefix + 1 + (offset - self.prefix - 1) % self.tail)
        } else {
            None
        }
    }

    fn record(&mut self, offset: usize, trail: bool) {
        let Some(slot) = self.slot(offset) else {
            return;
        };
        if self.trails.is_empty() {
            if !trail {
                return;
            }
            self.trails
                .resize((self.prefix + self.tail + 1).div_ceil(8), 0);
        }
        let bit = 1 << (slot % 8);
        if trail {
            self.trails[slot / 8] |= bit;
        } else {
            self.trails[slot / 8] &= !bit;
        }
    }

    pub(super) fn append(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            let offset = self.total;
            self.total += 1;
            if let Mode::Utf16 { big_endian } = self.mode {
                if let Some(first) = self.unit.take() {
                    let unit = if big_endian {
                        u16::from_be_bytes([first, byte])
                    } else {
                        u16::from_le_bytes([first, byte])
                    };
                    self.record(
                        offset - 1,
                        self.surrogate && (0xdc00..=0xdfff).contains(&unit),
                    );
                    self.record(offset, true);
                    self.surrogate = (0xd800..=0xdbff).contains(&unit);
                } else {
                    self.unit = Some(byte);
                    self.record(offset, self.surrogate);
                }
                continue;
            }
            if !self.utf8
                && let Some(code_page) = self.code_page
            {
                self.record(offset, self.lead);
                self.lead = !self.lead && code_page.lead(byte);
            }
            match self.mode {
                Mode::Detect => {
                    self.probe.push(byte);
                    match self.probe.as_slice() {
                        [0xff, 0xfe] | [0xfe, 0xff] => {
                            self.mode = Mode::Utf16 {
                                big_endian: byte == 0xff,
                            };
                            self.record(0, false);
                            self.record(1, true);
                            self.probe.clear();
                        }
                        [0xef, 0xbb, 0xbf] => {
                            self.mode = Mode::Utf8 { bom: true };
                            self.utf8 = true;
                            self.trails = Vec::new();
                            self.probe.clear();
                        }
                        [0xef] | [0xff] | [0xfe] | [0xef, 0xbb] => {}
                        _ => {
                            self.mode = Mode::Utf8 { bom: false };
                            self.check_utf8();
                        }
                    }
                }
                Mode::Utf8 { bom: false }
                    if !self.utf8
                        && self.code_page.is_some()
                        && (!byte.is_ascii() || !self.probe.is_empty()) =>
                {
                    self.probe.push(byte);
                    self.check_utf8();
                }
                _ => {}
            }
        }
    }

    fn check_utf8(&mut self) {
        match std::str::from_utf8(&self.probe) {
            Ok(_) => {
                self.utf8 |= self.probe.iter().any(|byte| !byte.is_ascii());
                if self.utf8 {
                    self.trails = Vec::new();
                }
                self.probe.clear();
            }
            Err(error) if error.error_len().is_some() => {
                if self.code_page.is_some() {
                    self.mode = Mode::Legacy;
                }
                self.probe.clear();
            }
            Err(_) => {}
        }
    }

    pub(super) fn finish(&mut self) {
        if !self.utf8
            && matches!(self.mode, Mode::Detect | Mode::Utf8 { bom: false })
            && !self.probe.is_empty()
            && self
                .code_page
                .filter(|page| page.single_byte)
                .and_then(|page| page.decode(&self.probe))
                .is_some()
        {
            self.mode = Mode::Legacy;
            self.probe.clear();
        }
    }

    pub(super) fn transcoded(&self) -> bool {
        matches!(
            self.mode,
            Mode::Legacy | Mode::Utf16 { .. } | Mode::Utf8 { bom: true }
        )
    }

    pub(super) fn native(&self) -> bool {
        matches!(self.mode, Mode::Legacy | Mode::Utf16 { .. })
    }

    fn pending(&self) -> usize {
        match self.mode {
            Mode::Legacy => usize::from(self.lead),
            Mode::Utf16 { .. } => {
                usize::from(self.unit.is_some()) + 2 * usize::from(self.surrogate)
            }
            _ => 0,
        }
    }

    pub(super) fn split(&self, offset: usize) -> bool {
        if !self.native() || offset > self.total {
            return false;
        }
        if offset == self.total {
            return self.pending() > 0;
        }
        self.slot(offset)
            .and_then(|slot| {
                self.trails
                    .get(slot / 8)
                    .map(|bits| bits & (1 << (slot % 8)) != 0)
            })
            .unwrap_or(false)
    }

    pub(super) fn bounds(
        &self,
        bytes: &[u8],
        offset: usize,
        complete: bool,
    ) -> Option<Range<usize>> {
        if matches!(self.mode, Mode::Detect) && !complete {
            return Some(0..0);
        }
        if !self.native() {
            return None;
        }
        let mut start = 0;
        while start < bytes.len() && self.split(offset + start) {
            start += 1;
        }
        let mut end = bytes.len();
        if !complete {
            if offset + end == self.total {
                end = end.saturating_sub(self.pending());
            } else {
                while end > start && self.split(offset + end) {
                    end -= 1;
                }
            }
        }
        Some(start.min(end)..end)
    }

    pub(super) fn page(
        &self,
        bytes: &[u8],
        offset: usize,
        limit: usize,
        complete: bool,
    ) -> Result<Range<usize>, sailry_protocol::Fault> {
        if matches!(self.mode, Mode::Detect) && !complete {
            return Ok(0..0);
        }
        if !self.native() {
            if offset > 0 && bytes.first().is_some_and(|byte| byte & 0xc0 == 0x80) {
                return Err(sailry_protocol::Fault::invalid(
                    "command output offset splits a UTF-8 character",
                ));
            }
            let end = if !complete && offset + bytes.len() == self.total {
                super::super::edges(bytes, bytes.len(), 0).0
            } else {
                bytes.len()
            };
            return super::super::utf8_page(&bytes[..end], 0, limit);
        }
        if self.split(offset) && offset < self.total {
            return Err(sailry_protocol::Fault::invalid(
                "command output offset splits an encoded character",
            ));
        }
        let available = if !complete && offset + bytes.len() == self.total {
            bytes.len().saturating_sub(self.pending())
        } else {
            bytes.len()
        };
        let mut end = available.min(limit);
        while end > 0 && end < available && self.split(offset + end) {
            end -= 1;
        }
        if end == 0 && available > 0 {
            end = 1;
            while end < available && self.split(offset + end) {
                end += 1;
            }
        }
        Ok(0..end)
    }

    pub(super) fn decode(&self, bytes: &[u8], offset: usize) -> (String, bool) {
        match self.mode {
            Mode::Legacy => {
                if let Some(text) = self.code_page.and_then(|page| page.decode(bytes)) {
                    return (text, false);
                }
                return (String::from_utf8_lossy(bytes).into_owned(), true);
            }
            Mode::Utf16 { big_endian } => {
                let bytes = if offset == 0 && bytes.len() >= 2 {
                    &bytes[2..]
                } else {
                    bytes
                };
                let wide: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| {
                        if big_endian {
                            u16::from_be_bytes(*pair)
                        } else {
                            u16::from_le_bytes(*pair)
                        }
                    })
                    .collect();
                let mut text = String::from_utf16_lossy(&wide);
                let invalid = bytes.len() % 2 != 0 || String::from_utf16(&wide).is_err();
                if bytes.len() % 2 != 0 {
                    text.push('\u{fffd}');
                }
                return (text, invalid);
            }
            Mode::Utf8 { bom: true } if offset == 0 && bytes.starts_with(&[0xef, 0xbb, 0xbf]) => {
                let bytes = &bytes[3..];
                return (
                    String::from_utf8_lossy(bytes).into_owned(),
                    std::str::from_utf8(bytes).is_err(),
                );
            }
            _ => {}
        }
        (
            String::from_utf8_lossy(bytes).into_owned(),
            std::str::from_utf8(bytes).is_err(),
        )
    }
}

#[cfg(test)]
mod tests;
