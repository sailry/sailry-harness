use serde::Serialize;
use std::io;

#[derive(Clone, Copy, Serialize)]
pub(super) struct Sample {
    pub current_rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub threads: u64,
}

#[cfg(target_os = "linux")]
pub(super) fn sample() -> io::Result<Sample> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    parse(&status)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn sample() -> io::Result<Sample> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "memory acceptance requires Linux /proc",
    ))
}

#[cfg(any(target_os = "linux", test))]
fn parse(status: &str) -> io::Result<Sample> {
    let field = |name: &str, unit: Option<&str>| -> io::Result<u64> {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "process status field is missing",
                )
            })?;
        let mut values = value.split_whitespace();
        let number = values
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "process status field is invalid",
                )
            })?;
        if values.next() != unit || values.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "process status unit is invalid",
            ));
        }
        if unit.is_some() {
            number.checked_mul(1024).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "process memory value is too large",
                )
            })
        } else {
            Ok(number)
        }
    };
    Ok(Sample {
        current_rss_bytes: field("VmRSS:", Some("kB"))?,
        peak_rss_bytes: field("VmHWM:", Some("kB"))?,
        threads: field("Threads:", None)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_resident_and_virtual_memory() {
        let sample =
            parse("VmPeak: 900000 kB\nVmRSS: 125 kB\nVmHWM: 150 kB\nThreads: 4\n").unwrap();
        assert_eq!(sample.current_rss_bytes, 128000);
        assert_eq!(sample.peak_rss_bytes, 153600);
        assert_eq!(sample.threads, 4);
    }

    #[test]
    fn rejects_missing_or_invalid_values() {
        for status in [
            "",
            "VmRSS: 3 MB\nVmHWM: 4 kB\nThreads: 2\n",
            "VmRSS: 18446744073709551615 kB\nVmHWM: 4 kB\nThreads: 2\n",
        ] {
            assert!(parse(status).is_err());
        }
    }
}
