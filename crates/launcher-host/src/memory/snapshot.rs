//! Memory readings and the one log line they are reported in.
//!
//! Nothing here reads the system: callers pass the text of the kernel's files, the platform's
//! figures and the byte counters their owners already keep.

use std::fmt::Write as _;

/// Begins every memory line, so reports and tools can find them in a client log. The Android
/// exit report finds them by `memory_line` in `packaging/android/runtime.json`, which a test
/// keeps equal to this.
pub const LINE_PREFIX: &str = "memory event=";

/// Bytes the kernel charges to one process; a figure it did not expose stays `None`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessMemory {
    /// Resident set size: every page in RAM, shared ones counted in full.
    pub resident: Option<u64>,
    /// Proportional set size: resident pages, shared ones divided among their users.
    pub proportional: Option<u64>,
    /// Pages moved to swap, which on phones is compressed RAM.
    pub swapped: Option<u64>,
}

impl ProcessMemory {
    /// Reads `Rss`, `Pss` and `Swap` from the text of `/proc/<pid>/smaps_rollup`.
    #[must_use]
    pub fn from_smaps_rollup(text: &str) -> Self {
        Self {
            resident: kib_field(text, "Rss:"),
            proportional: kib_field(text, "Pss:"),
            swapped: kib_field(text, "Swap:"),
        }
    }

    /// Reads `VmRSS` and `VmSwap` from the text of `/proc/<pid>/status`, the kernel's running
    /// counters; it has no proportional size.
    #[must_use]
    pub fn from_status(text: &str) -> Self {
        Self {
            resident: kib_field(text, "VmRSS:"),
            proportional: None,
            swapped: kib_field(text, "VmSwap:"),
        }
    }

    /// The same figures, with `resident` taken from the text of `/proc/<pid>/statm` when the
    /// rollup did not give one. `page_bytes` is the kernel's page size.
    #[must_use]
    pub fn or_statm(mut self, statm: &str, page_bytes: u64) -> Self {
        if self.resident.is_none() {
            self.resident = statm_resident_bytes(statm, page_bytes);
        }
        self
    }

    /// Whether the kernel exposed no figure at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The resident set size in `/proc/<pid>/statm`, whose second field counts resident pages.
#[must_use]
pub fn statm_resident_bytes(statm: &str, page_bytes: u64) -> Option<u64> {
    statm
        .split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?
        .checked_mul(page_bytes)
}

/// The value of a `/proc` line such as `MemTotal:  16318060 kB`, in bytes. `key` includes the
/// colon, so `Pss:` never matches `Pss_Anon:` and `Swap:` never matches `SwapPss:`.
#[must_use]
pub fn kib_field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(key))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

/// The device's memory as `/proc/meminfo` reports it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemMemory {
    pub total: Option<u64>,
    /// The kernel's estimate of what new work can use without swapping.
    pub available: Option<u64>,
    pub swap_total: Option<u64>,
    pub swap_free: Option<u64>,
}

impl SystemMemory {
    /// Reads the totals from the text of `/proc/meminfo`.
    #[must_use]
    pub fn from_meminfo(text: &str) -> Self {
        Self {
            total: kib_field(text, "MemTotal:"),
            available: kib_field(text, "MemAvailable:"),
            swap_total: kib_field(text, "SwapTotal:"),
            swap_free: kib_field(text, "SwapFree:"),
        }
    }
}

/// Android's own view, from `ActivityManager.MemoryInfo`: what it counts as available, the
/// level below which it starts ending background work, and whether it is below that level now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformMemory {
    pub available: u64,
    pub threshold: u64,
    pub low: bool,
}

/// One helper process this client started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildMemory {
    /// The kernel's short name for the process.
    pub name: String,
    pub pid: u32,
    pub memory: ProcessMemory,
}

/// Totals the client's subsystems already track, reported beside the kernel's figures.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Counters {
    /// Named byte totals, such as a cache's size.
    pub pools: Vec<(&'static str, u64)>,
    /// Named plain counts, such as resident chunk sections.
    pub counts: Vec<(&'static str, u64)>,
}

impl Counters {
    /// No counters, for a static.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pools: Vec::new(),
            counts: Vec::new(),
        }
    }
}

/// Everything one memory line reports.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub process: ProcessMemory,
    pub children: Vec<ChildMemory>,
    pub system: SystemMemory,
    pub platform: Option<PlatformMemory>,
    pub counters: Counters,
}

impl Snapshot {
    /// The log line for `event`: sizes in whole MiB, groups and figures that are unknown left
    /// out. For example:
    ///
    /// `memory event=world_entry rss=812M pss=640M swap=12M children=[bedrock_core:71 rss=38M]
    /// system=[available=1210M total=5365M] android=[available=1180M threshold=216M low=false]
    /// pools=[chunk_arena=48M] counts=[sections=1234]`
    #[must_use]
    pub fn line(&self, event: &str) -> String {
        let mut line = String::from(LINE_PREFIX);
        line.extend(event.chars().map(|character| {
            if character.is_whitespace() {
                '_'
            } else {
                character
            }
        }));
        push_process(&mut line, &self.process);
        if !self.children.is_empty() {
            line.push_str(" children=[");
            for (index, child) in self.children.iter().enumerate() {
                if index > 0 {
                    line.push_str("; ");
                }
                let _ = write!(line, "{}:{}", token(&child.name), child.pid);
                push_process(&mut line, &child.memory);
            }
            line.push(']');
        }
        push_group(
            &mut line,
            "system",
            &[
                ("available", self.system.available),
                ("total", self.system.total),
                ("swap_free", self.system.swap_free),
                ("swap_total", self.system.swap_total),
            ],
        );
        if let Some(platform) = self.platform {
            let _ = write!(
                line,
                " android=[available={} threshold={} low={}]",
                mib(platform.available),
                mib(platform.threshold),
                platform.low
            );
        }
        let pools: Vec<_> = self
            .counters
            .pools
            .iter()
            .map(|(name, bytes)| (*name, Some(*bytes)))
            .collect();
        push_group(&mut line, "pools", &pools);
        if !self.counters.counts.is_empty() {
            line.push_str(" counts=[");
            for (index, (name, count)) in self.counters.counts.iter().enumerate() {
                if index > 0 {
                    line.push(' ');
                }
                let _ = write!(line, "{name}={count}");
            }
            line.push(']');
        }
        line
    }
}

/// Appends the process figures the kernel exposed.
fn push_process(line: &mut String, memory: &ProcessMemory) {
    for (key, bytes) in [
        ("rss", memory.resident),
        ("pss", memory.proportional),
        ("swap", memory.swapped),
    ] {
        if let Some(bytes) = bytes {
            let _ = write!(line, " {key}={}", mib(bytes));
        }
    }
}

/// Appends ` name=[key=1M ...]` with the known sizes, or nothing when none is known.
fn push_group(line: &mut String, name: &str, sizes: &[(&str, Option<u64>)]) {
    let mut known = sizes
        .iter()
        .filter_map(|(key, bytes)| bytes.map(|bytes| (key, bytes)))
        .peekable();
    if known.peek().is_none() {
        return;
    }
    let _ = write!(line, " {name}=[");
    for (index, (key, bytes)) in known.enumerate() {
        if index > 0 {
            line.push(' ');
        }
        let _ = write!(line, "{key}={}", mib(bytes));
    }
    line.push(']');
}

/// `bytes` rounded to whole MiB, as `12M`.
fn mib(bytes: u64) -> String {
    format!("{}M", bytes.saturating_add(1 << 19) >> 20)
}

/// A process name that stays one token inside a bracketed group.
fn token(name: &str) -> String {
    let token: String = name
        .chars()
        .map(|character| match character {
            character if character.is_whitespace() => '_',
            '[' | ']' | ';' | ':' | '=' => '_',
            character => character,
        })
        .collect();
    if token.is_empty() {
        "process".to_owned()
    } else {
        token
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
