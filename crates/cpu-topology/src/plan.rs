//! The pool-size and placement policy decided from a topology.

use std::fmt;

use crate::overrides;
use crate::topology::{BackgroundPlacement, CpuTopology};

/// Background CPUs left to threads that work beside confined pools without being confined
/// themselves, such as the world-stream service the frame waits on.
const SERVICE_CPUS: usize = 1;
/// Where Linux and Android describe their CPUs.
#[cfg(any(target_os = "linux", target_os = "android"))]
const SYSFS_CPU_ROOT: &str = "/sys/devices/system/cpu";

/// The logical processors of a machine whose kernel reports `online` CPUs, seen from a thread
/// whose affinity mask admits `allowed` of them: the online count when it is readable, and
/// never fewer than the thread already has.
#[cfg(any(target_os = "android", test))]
pub(crate) fn machine_cores(allowed: usize, online: Option<usize>) -> usize {
    online.map_or(allowed, |online| online.max(allowed))
}

/// Whether the core topology shapes thread pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyMode {
    /// Applied on Android only.
    Auto,
    On,
    Off,
}

impl TopologyMode {
    /// Reads an override value: `1`/`on`/`true` or `0`/`off`/`false`, any case; anything else
    /// is [`Self::Auto`].
    pub fn parse(value: Option<&str>) -> Self {
        match value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("1" | "on" | "true") => Self::On,
            Some("0" | "off" | "false") => Self::Off,
            _ => Self::Auto,
        }
    }

    /// Whether the mode applies the topology on a platform that is or is not Android.
    pub const fn applies(self, android: bool) -> bool {
        match self {
            Self::Auto => android,
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// The cores thread pools size themselves for and where background threads run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuPlan {
    logical: usize,
    mode: TopologyMode,
    /// Whether `mode` applies the topology on this platform.
    applies: bool,
    topology: Option<CpuTopology>,
    placement: Option<BackgroundPlacement>,
    shared_pool_cores: usize,
}

impl CpuPlan {
    /// Decides the plan for `logical` processors (already capped by `CINNABAR_CPU_CORES`) and
    /// the topology of the CPUs the process may use. When `mode` applies on this platform, a
    /// topology with clear speed tiers reserves frame cores; otherwise the plan reserves
    /// nothing and every pool sizes for `logical`.
    pub fn new(
        logical: usize,
        topology: Option<CpuTopology>,
        mode: TopologyMode,
        android: bool,
    ) -> Self {
        let logical = logical.max(1);
        let applies = mode.applies(android);
        let placement = topology
            .as_ref()
            .filter(|_| applies)
            .and_then(CpuTopology::placement);
        let shared_pool_cores = match (&placement, &topology) {
            (Some(_), Some(topology)) => topology.weighted_cores().clamp(1, logical),
            _ => logical,
        };
        Self {
            logical,
            mode,
            applies,
            topology,
            placement,
            shared_pool_cores,
        }
    }

    /// Reads the machine: the logical-processor count, the overrides, and the topology.
    ///
    /// Linux plans with the CPUs the calling thread may run on, so a container's share is the
    /// machine. Android plans with every CPU, for the count as well as the topology: it moves
    /// the app between cpusets, so the mask at startup says little about the cores the app
    /// gets in the foreground, and an affinity request naming a CPU outside the current cpuset
    /// still applies to the rest. The plan is decided once, so a count taken from a narrow
    /// startup mask would undersize every pool for the life of the process.
    pub(crate) fn detect() -> Self {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        #[cfg(target_os = "android")]
        let available = machine_cores(
            available,
            crate::topology::online_cpus(std::path::Path::new(SYSFS_CPU_ROOT)),
        );
        let logical =
            overrides::capped_cores(available, overrides::value(overrides::CORES_ENV).as_deref());
        let mode = TopologyMode::parse(overrides::value(overrides::TOPOLOGY_ENV).as_deref());
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let topology = CpuTopology::read(std::path::Path::new(SYSFS_CPU_ROOT));
        #[cfg(target_os = "linux")]
        let topology = topology.and_then(|all| match crate::affinity::current_cpus() {
            Some(allowed) => all.restricted_to(&allowed),
            None => Some(all),
        });
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let topology = None;
        Self::new(logical, topology, mode, cfg!(target_os = "android"))
    }

    /// Logical processors the machine offers, or fewer under the `CINNABAR_CPU_CORES` cap.
    pub const fn logical(&self) -> usize {
        self.logical
    }

    /// Logical processors that pools free to run on any core size themselves for: the CPUs'
    /// combined speed in fastest cores when frame cores are reserved, else [`Self::logical`].
    /// A phone with one prime, three middle and four LITTLE cores sizes these pools like a
    /// five-core machine.
    pub const fn shared_pool_cores(&self) -> usize {
        self.shared_pool_cores
    }

    /// Most threads a pool confined by [`crate::enter_background_cores`] should run: one per
    /// background CPU, less the CPUs left to unconfined service threads. `None` when no frame
    /// cores are reserved, so the pool's own policy decides alone.
    pub fn background_width(&self) -> Option<usize> {
        let placement = self.placement.as_ref()?;
        Some(
            placement
                .background
                .len()
                .saturating_sub(SERVICE_CPUS)
                .max(1),
        )
    }

    /// The detected topology, applied or not.
    pub const fn topology(&self) -> Option<&CpuTopology> {
        self.topology.as_ref()
    }

    /// The frame and background CPUs, when the plan keeps background threads off frame cores.
    pub const fn placement(&self) -> Option<&BackgroundPlacement> {
        self.placement.as_ref()
    }

    /// The override that chose whether to apply the topology.
    pub const fn mode(&self) -> TopologyMode {
        self.mode
    }
}

impl fmt::Display for CpuPlan {
    /// Writes the plan for the startup log, e.g. `8 logical cores, cpu_capacity 1024x2[6,7]
    /// 380x6[0,1,2,3,4,5], frame cpus [6, 7], background cpus [0, 1, 2, 3, 4, 5]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} logical cores, ", self.logical)?;
        let Some(topology) = &self.topology else {
            return write!(f, "topology unreadable");
        };
        write!(f, "{topology}, ")?;
        match &self.placement {
            Some(placement) => {
                let mut frame = placement.frame.clone();
                frame.sort_unstable();
                write!(
                    f,
                    "frame cpus {frame:?}, background cpus {:?}",
                    placement.background
                )
            }
            None if !self.applies => {
                write!(f, "topology not applied")
            }
            None => write!(f, "no frame cpus reserved"),
        }
    }
}
