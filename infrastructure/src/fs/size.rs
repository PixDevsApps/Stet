use std::fmt;

pub const MIB: u64 = 1024 * 1024;

/// When a file opens in large-file mode and when it is refused (PRD; technical critique #15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizePolicy {
    /// Files from this size open in large-file mode.
    pub large_from: u64,
    /// Files from this size are refused.
    pub refuse_from: u64,
    /// Files are also refused when less than this many times their size is available in
    /// memory (`MemAvailable`).
    pub ram_factor: u64,
}

impl Default for SizePolicy {
    fn default() -> Self {
        Self {
            large_from: 50 * MIB,
            refuse_from: 256 * MIB,
            ram_factor: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeClass {
    Normal,
    Large,
    TooLarge,
}

/// Why a file is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The file is at least `limit` bytes.
    Size { limit: u64 },
    /// Opening needs about `needed` bytes of memory and only `available` are free.
    Memory { needed: u64, available: u64 },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Size { limit } => {
                write!(f, "Stet opens files smaller than {}", human_size(limit))
            }
            Self::Memory { needed, available } => write!(
                f,
                "it needs about {} of free memory and {} is available",
                human_size(needed),
                human_size(available)
            ),
        }
    }
}

impl SizePolicy {
    /// Classifies a file of `size` bytes; `mem_available` is `MemAvailable` in bytes, `None`
    /// when unknown.
    pub fn classify(&self, size: u64, mem_available: Option<u64>) -> SizeClass {
        if self.refusal(size, mem_available).is_some() {
            SizeClass::TooLarge
        } else if size >= self.large_from {
            SizeClass::Large
        } else {
            SizeClass::Normal
        }
    }

    pub fn refusal(&self, size: u64, mem_available: Option<u64>) -> Option<Refusal> {
        if size >= self.refuse_from {
            return Some(Refusal::Size {
                limit: self.refuse_from,
            });
        }
        let needed = size.saturating_mul(self.ram_factor);
        match mem_available {
            Some(available) if needed > available => Some(Refusal::Memory { needed, available }),
            _ => None,
        }
    }
}

/// `MemAvailable` from `/proc/meminfo`, in bytes.
pub fn mem_available() -> Option<u64> {
    parse_mem_available(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

fn parse_mem_available(meminfo: &str) -> Option<u64> {
    let value = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let kib: u64 = value.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kib.checked_mul(1024)
}

/// Sizes for messages: bytes, KiB or MiB with one decimal.
pub(crate) fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..MIB => format!("{:.1} KiB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MiB", bytes as f64 / MIB as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLENTY: Option<u64> = Some(64 * 1024 * MIB);

    #[test]
    fn default_thresholds_match_the_prd() {
        let policy = SizePolicy::default();
        assert_eq!(policy.classify(0, PLENTY), SizeClass::Normal);
        assert_eq!(policy.classify(50 * MIB - 1, PLENTY), SizeClass::Normal);
        assert_eq!(policy.classify(50 * MIB, PLENTY), SizeClass::Large);
        assert_eq!(policy.classify(256 * MIB - 1, PLENTY), SizeClass::Large);
        assert_eq!(policy.classify(256 * MIB, PLENTY), SizeClass::TooLarge);
        assert_eq!(
            policy.refusal(300 * MIB, PLENTY),
            Some(Refusal::Size { limit: 256 * MIB })
        );
    }

    #[test]
    fn little_free_memory_refuses_what_would_otherwise_open() {
        let policy = SizePolicy::default();
        let available = Some(399 * MIB);
        assert_eq!(policy.classify(99 * MIB, available), SizeClass::Large);
        assert_eq!(policy.classify(100 * MIB, available), SizeClass::TooLarge);
        assert_eq!(
            policy.refusal(100 * MIB, available),
            Some(Refusal::Memory {
                needed: 400 * MIB,
                available: 399 * MIB
            })
        );
        assert_eq!(
            policy.classify(10 * MIB, Some(39 * MIB)),
            SizeClass::TooLarge
        );
    }

    #[test]
    fn unknown_free_memory_only_applies_the_cap() {
        let policy = SizePolicy::default();
        assert_eq!(policy.classify(200 * MIB, None), SizeClass::Large);
        assert_eq!(policy.classify(256 * MIB, None), SizeClass::TooLarge);
    }

    #[test]
    fn refusals_explain_themselves() {
        assert_eq!(
            Refusal::Size { limit: 256 * MIB }.to_string(),
            "Stet opens files smaller than 256.0 MiB"
        );
        assert_eq!(
            Refusal::Memory {
                needed: 400 * MIB,
                available: 3 * MIB / 2
            }
            .to_string(),
            "it needs about 400.0 MiB of free memory and 1.5 MiB is available"
        );
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human_size(12), "12 bytes");
        assert_eq!(human_size(4096), "4.0 KiB");
        assert_eq!(human_size(300 * MIB), "300.0 MiB");
    }

    #[test]
    fn parses_mem_available() {
        let meminfo = "MemTotal:       65755292 kB\nMemFree:        20000000 kB\nMemAvailable:   41234567 kB\n";
        assert_eq!(parse_mem_available(meminfo), Some(41_234_567 * 1024));
        assert_eq!(parse_mem_available("MemTotal: 1 kB\n"), None);
        assert_eq!(parse_mem_available("MemAvailable: lots\n"), None);
    }

    #[test]
    fn reads_this_machines_mem_available() {
        if std::path::Path::new("/proc/meminfo").exists() {
            assert!(mem_available().is_some_and(|bytes| bytes > 0));
        }
    }
}
