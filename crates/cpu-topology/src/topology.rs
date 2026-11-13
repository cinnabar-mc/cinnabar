//! Relative core speeds read from Linux sysfs, grouped into speed tiers.

use std::fmt;
use std::path::Path;

use crate::FRAME_CORES;

/// A core joins the current tier while its speed is at least this many sixteenths of the
/// tier's fastest core, so small per-core binning differences do not split one cluster.
const TIER_SIXTEENTHS: u64 = 15;
/// Background threads need at least this many CPUs left after the frame cores are reserved;
/// otherwise the topology is left alone, so a small machine never squeezes all its world
/// work onto one or two slow cores.
const MIN_BACKGROUND_CPUS: usize = 4;

/// Where a topology's speeds came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedSource {
    /// The scheduler's `cpu_capacity`, normalised so the fastest core is 1024.
    Capacity,
    /// `cpufreq/cpuinfo_max_freq`, which overstates slow core designs that clock high.
    MaxFrequency,
}

/// Relative speeds of the logical CPUs a process may run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuTopology {
    source: SpeedSource,
    /// `(cpu, speed)` pairs, fastest first; equal speeds put the higher CPU number first, since
    /// phones number their prime cores last.
    speeds: Vec<(usize, u64)>,
}

/// CPUs reserved for the main and render threads, and the CPUs background threads may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundPlacement {
    /// The fastest non-LITTLE CPUs, at most [`FRAME_CORES`] of them.
    pub frame: Vec<usize>,
    /// Every other CPU, in ascending order.
    pub background: Vec<usize>,
}

impl CpuTopology {
    /// Reads every `cpuN` directory below `root`, normally `/sys/devices/system/cpu`. Uses
    /// `cpu_capacity` when every CPU has a positive one, else `cpufreq/cpuinfo_max_freq` under
    /// the same condition; returns `None` when neither covers every CPU.
    pub fn read(root: &Path) -> Option<Self> {
        let cpus = cpu_numbers(root)?;
        [
            (SpeedSource::Capacity, "cpu_capacity"),
            (SpeedSource::MaxFrequency, "cpufreq/cpuinfo_max_freq"),
        ]
        .into_iter()
        .find_map(|(source, file)| {
            let speeds = cpus
                .iter()
                .map(|&cpu| {
                    read_speed(&root.join(format!("cpu{cpu}")).join(file)).map(|speed| (cpu, speed))
                })
                .collect::<Option<Vec<_>>>()?;
            Self::new(source, speeds)
        })
    }

    /// Builds a topology from `(cpu, speed)` pairs that name each CPU once; `None` when empty
    /// or any speed is zero.
    fn new(source: SpeedSource, mut speeds: Vec<(usize, u64)>) -> Option<Self> {
        if speeds.is_empty() || speeds.iter().any(|&(_, speed)| speed == 0) {
            return None;
        }
        speeds.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        Some(Self { source, speeds })
    }

    /// Keeps only `allowed` CPUs, such as the process's affinity mask; `None` if none remain.
    #[must_use]
    pub fn restricted_to(&self, allowed: &[usize]) -> Option<Self> {
        let speeds = self
            .speeds
            .iter()
            .copied()
            .filter(|(cpu, _)| allowed.contains(cpu))
            .collect();
        Self::new(self.source, speeds)
    }

    /// Where the speeds came from.
    pub const fn source(&self) -> SpeedSource {
        self.source
    }

    /// CPU numbers grouped by speed tier, fastest tier first.
    pub fn tiers(&self) -> Vec<Vec<usize>> {
        self.ranked_tiers()
            .into_iter()
            .map(|(_, cpus)| cpus)
            .collect()
    }

    /// Each tier's fastest speed with its CPUs in rank order, fastest tier first.
    fn ranked_tiers(&self) -> Vec<(u64, Vec<usize>)> {
        let mut tiers: Vec<(u64, Vec<usize>)> = Vec::new();
        for &(cpu, speed) in &self.speeds {
            match tiers.last_mut() {
                Some((top, cpus)) if speed * 16 >= *top * TIER_SIXTEENTHS => cpus.push(cpu),
                _ => tiers.push((speed, vec![cpu])),
            }
        }
        tiers
    }

    /// Whether every CPU falls into one speed tier.
    pub fn is_uniform(&self) -> bool {
        self.tiers().len() == 1
    }

    /// The fastest tier's CPUs.
    pub fn fastest(&self) -> Vec<usize> {
        self.tiers().swap_remove(0)
    }

    /// The slowest tier's CPUs, or none on a uniform machine.
    pub fn little(&self) -> Vec<usize> {
        let mut tiers = self.tiers();
        if tiers.len() > 1 {
            tiers.pop().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Every CPU outside the slowest tier, fastest first; every CPU on a uniform machine.
    pub fn big(&self) -> Vec<usize> {
        let little = self.little();
        self.speeds
            .iter()
            .map(|&(cpu, _)| cpu)
            .filter(|cpu| !little.contains(cpu))
            .collect()
    }

    /// The CPUs' combined speed in units of the fastest core, rounded up.
    pub fn weighted_cores(&self) -> usize {
        let fastest = u128::from(self.speeds[0].1);
        let total: u128 = self
            .speeds
            .iter()
            .map(|&(_, speed)| u128::from(speed))
            .sum();
        usize::try_from(total.div_ceil(fastest)).unwrap_or(usize::MAX)
    }

    /// Reserves the fastest non-LITTLE CPUs for the frame threads. `None` on a uniform machine,
    /// or when fewer than [`MIN_BACKGROUND_CPUS`] would remain for background threads.
    pub fn placement(&self) -> Option<BackgroundPlacement> {
        if self.is_uniform() {
            return None;
        }
        let frame: Vec<usize> = self.big().into_iter().take(FRAME_CORES).collect();
        let mut background: Vec<usize> = self
            .speeds
            .iter()
            .map(|&(cpu, _)| cpu)
            .filter(|cpu| !frame.contains(cpu))
            .collect();
        if background.len() < MIN_BACKGROUND_CPUS {
            return None;
        }
        background.sort_unstable();
        Some(BackgroundPlacement { frame, background })
    }
}

impl fmt::Display for CpuTopology {
    /// Writes the source, then each tier as `speed`x`count[cpus]`, fastest first, e.g.
    /// `cpu_capacity 1024x1[7] 280x4[0,1,2,3]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match self.source {
            SpeedSource::Capacity => "cpu_capacity",
            SpeedSource::MaxFrequency => "cpuinfo_max_freq",
        };
        write!(f, "{source}")?;
        for (top, mut cpus) in self.ranked_tiers() {
            cpus.sort_unstable();
            let list = cpus
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            write!(f, " {top}x{}[{list}]", cpus.len())?;
        }
        Ok(())
    }
}

/// How many CPUs the kernel has online, from the `online` list below `root`, such as `0-7` or
/// `0-3,6-7`; `None` if the file is missing, empty or malformed. Unlike a thread's affinity
/// mask, the list does not depend on the caller or its cpuset.
#[cfg(any(target_os = "android", test))]
pub(crate) fn online_cpus(root: &Path) -> Option<usize> {
    let list = std::fs::read_to_string(root.join("online")).ok()?;
    list.trim().split(',').try_fold(0_usize, |count, range| {
        let (first, last) = range.split_once('-').unwrap_or((range, range));
        let (first, last) = (first.parse::<usize>().ok()?, last.parse::<usize>().ok()?);
        count.checked_add(last.checked_sub(first)?.checked_add(1)?)
    })
}

/// The `N` of every `cpuN` entry below `root`, ascending; `None` if `root` cannot be listed.
fn cpu_numbers(root: &Path) -> Option<Vec<usize>> {
    let mut cpus: Vec<usize> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            let digits = name.to_str()?.strip_prefix("cpu")?;
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok()
        })
        .collect();
    cpus.sort_unstable();
    (!cpus.is_empty()).then_some(cpus)
}

/// A positive integer read from a sysfs file; `None` if missing, unreadable or malformed.
fn read_speed(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|&speed| speed > 0)
}
