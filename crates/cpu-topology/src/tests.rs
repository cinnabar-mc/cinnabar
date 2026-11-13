use std::path::Path;

use crate::overrides::{CORES_ENV, TOPOLOGY_ENV, android_property, capped_cores};
use crate::plan::machine_cores;
use crate::topology::online_cpus;
use crate::{BackgroundPlacement, CpuPlan, CpuTopology, SpeedSource, TopologyMode};

/// Writes a fake `/sys/devices/system/cpu` tree: one `(capacity, max_freq)` pair per CPU, where
/// `None` leaves that file out.
fn sysfs(cpus: &[(Option<u64>, Option<u64>)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    // Entries the reader must skip.
    std::fs::create_dir_all(root.path().join("cpufreq")).unwrap();
    std::fs::create_dir_all(root.path().join("cpuidle")).unwrap();
    std::fs::write(root.path().join("online"), "0-7\n").unwrap();
    for (cpu, &(capacity, max_freq)) in cpus.iter().enumerate() {
        let dir = root.path().join(format!("cpu{cpu}"));
        std::fs::create_dir_all(dir.join("cpufreq")).unwrap();
        if let Some(capacity) = capacity {
            std::fs::write(dir.join("cpu_capacity"), format!("{capacity}\n")).unwrap();
        }
        if let Some(max_freq) = max_freq {
            std::fs::write(
                dir.join("cpufreq/cpuinfo_max_freq"),
                format!("{max_freq}\n"),
            )
            .unwrap();
        }
    }
    root
}

/// Capacities only, numbered from cpu0.
fn capacities(values: &[u64]) -> tempfile::TempDir {
    sysfs(
        &values
            .iter()
            .map(|&value| (Some(value), None))
            .collect::<Vec<_>>(),
    )
}

fn read(root: &tempfile::TempDir) -> CpuTopology {
    CpuTopology::read(root.path()).unwrap()
}

/// Helio G99-like: two fast cores numbered last, six LITTLE cores.
const TWO_PLUS_SIX: [u64; 8] = [380, 380, 380, 380, 380, 380, 1024, 1024];
/// Snapdragon 7+ Gen 2-like: four LITTLE, three middle, one prime.
const ONE_THREE_FOUR: [u64; 8] = [280, 280, 280, 280, 870, 870, 870, 1024];

#[test]
fn uniform_cores_form_one_tier_and_reserve_nothing() {
    let topology = read(&capacities(&[1024; 8]));
    assert_eq!(topology.source(), SpeedSource::Capacity);
    assert!(topology.is_uniform());
    assert_eq!(topology.fastest().len(), 8);
    assert!(topology.little().is_empty());
    assert_eq!(topology.big().len(), 8);
    assert_eq!(topology.weighted_cores(), 8);
    assert_eq!(topology.placement(), None);
}

/// Small binning differences inside one cluster do not split it into tiers.
#[test]
fn near_equal_cores_stay_one_tier() {
    let topology = read(&capacities(&[1024, 1010, 1000, 990]));
    assert!(topology.is_uniform());
    assert_eq!(topology.placement(), None);
}

#[test]
fn four_big_four_little_reserves_two_big_cores() {
    let topology = read(&capacities(&[400, 400, 400, 400, 1024, 1024, 1024, 1024]));
    assert_eq!(topology.tiers(), [vec![7, 6, 5, 4], vec![3, 2, 1, 0]]);
    assert_eq!(topology.little(), [3, 2, 1, 0]);
    assert_eq!(topology.big(), [7, 6, 5, 4]);
    assert_eq!(
        topology.placement(),
        Some(BackgroundPlacement {
            frame: vec![7, 6],
            background: vec![0, 1, 2, 3, 4, 5],
        })
    );
    // 4 × 1024 + 4 × 400 = 5.6 fastest cores.
    assert_eq!(topology.weighted_cores(), 6);
}

#[test]
fn prime_middle_little_reserves_the_prime_and_one_middle_core() {
    let topology = read(&capacities(&ONE_THREE_FOUR));
    assert_eq!(topology.tiers(), [vec![7], vec![6, 5, 4], vec![3, 2, 1, 0]]);
    assert_eq!(topology.fastest(), [7]);
    assert_eq!(topology.big(), [7, 6, 5, 4]);
    assert_eq!(topology.little(), [3, 2, 1, 0]);
    assert_eq!(
        topology.placement(),
        Some(BackgroundPlacement {
            frame: vec![7, 6],
            background: vec![0, 1, 2, 3, 4, 5],
        })
    );
    // 1024 + 3 × 870 + 4 × 280 = 4.64 fastest cores.
    assert_eq!(topology.weighted_cores(), 5);
    assert_eq!(
        topology.to_string(),
        "cpu_capacity 1024x1[7] 870x3[4,5,6] 280x4[0,1,2,3]"
    );
}

/// A device without `cpu_capacity` falls back to each core's maximum frequency.
#[test]
fn missing_capacity_falls_back_to_max_frequency() {
    let mut cpus = vec![(None, Some(2_000_000)); 6];
    cpus.extend([(None, Some(2_200_000)); 2]);
    cpus[0].0 = Some(1024);
    let topology = read(&sysfs(&cpus));
    assert_eq!(topology.source(), SpeedSource::MaxFrequency);
    assert_eq!(topology.fastest(), [7, 6]);
    assert_eq!(topology.little().len(), 6);
}

#[test]
fn missing_or_unusable_files_give_no_topology() {
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(CpuTopology::read(empty.path()), None);
    assert_eq!(CpuTopology::read(Path::new("/nonexistent/cpu")), None);
    // One CPU lacks both files.
    let mut partial = vec![(Some(1024), Some(2_000_000)); 4];
    partial[2] = (None, None);
    assert_eq!(CpuTopology::read(sysfs(&partial).path()), None);
    // Zero and malformed values do not count as speeds.
    assert_eq!(CpuTopology::read(capacities(&[1024, 0]).path()), None);
    let malformed = capacities(&[1024, 512]);
    std::fs::write(malformed.path().join("cpu1/cpu_capacity"), "fast\n").unwrap();
    assert_eq!(CpuTopology::read(malformed.path()), None);
}

/// A process confined to some CPUs plans only with those.
#[test]
fn restriction_to_allowed_cpus_can_leave_a_uniform_set() {
    let topology = read(&capacities(&TWO_PLUS_SIX));
    assert_eq!(
        topology.restricted_to(&[0, 1, 2, 3]).unwrap().placement(),
        None
    );
    assert_eq!(topology.restricted_to(&[]), None);
}

/// Too few CPUs left after reserving the frame cores leaves the machine alone.
#[test]
fn frame_cores_are_reserved_only_with_four_background_cpus() {
    assert!(
        read(&capacities(&[400, 400, 400, 400, 1024, 1024]))
            .placement()
            .is_some()
    );
    assert_eq!(
        read(&capacities(&[400, 400, 400, 1024, 1024])).placement(),
        None
    );
    assert_eq!(read(&capacities(&[400, 400, 1024, 1024])).placement(), None);
}

/// One fast core among slow ones is the only frame core.
#[test]
fn a_single_fast_core_is_the_only_frame_core() {
    let placement = read(&capacities(&[300, 300, 300, 300, 300, 300, 300, 1024]))
        .placement()
        .unwrap();
    assert_eq!(placement.frame, [7]);
    assert_eq!(placement.background, [0, 1, 2, 3, 4, 5, 6]);
}

/// Two prime cores above six slightly slower ones still reserve the primes.
#[test]
fn two_prime_six_performance_reserves_the_primes() {
    let topology = read(&capacities(&[790, 790, 790, 790, 790, 790, 1024, 1024]));
    assert_eq!(topology.placement().unwrap().frame, [7, 6]);
    // 2 + 6 x 790/1024 = 6.6 fastest cores.
    assert_eq!(topology.weighted_cores(), 7);
}

fn plan(values: &[u64], mode: TopologyMode, android: bool) -> CpuPlan {
    let topology = read(&capacities(values));
    CpuPlan::new(values.len(), Some(topology), mode, android)
}

/// Android sizes unconfined pools by the cores' combined speed, keeps background threads off
/// the two fastest cores and fits confined pools to the background CPUs less one.
#[test]
fn android_plans_size_pools_by_speed_and_reserve_frame_cores() {
    let g99 = plan(&TWO_PLUS_SIX, TopologyMode::Auto, true);
    // 2 + 6 x 380/1024 = 4.2 fastest cores.
    assert_eq!(g99.shared_pool_cores(), 5);
    assert_eq!(g99.logical(), 8);
    assert_eq!(g99.background_width(), Some(5));
    assert_eq!(
        g99.placement()
            .map(|placement| placement.background.clone()),
        Some(vec![0, 1, 2, 3, 4, 5])
    );
    let prime = plan(&ONE_THREE_FOUR, TopologyMode::Auto, true);
    assert_eq!(prime.shared_pool_cores(), 5);
    assert_eq!(prime.background_width(), Some(5));
    let uniform = plan(&[1024; 8], TopologyMode::Auto, true);
    assert_eq!(uniform.shared_pool_cores(), 8);
    assert_eq!(uniform.placement(), None);
    assert_eq!(uniform.background_width(), None);
}

/// Desktop behaviour is unchanged unless the topology is forced on, and `off` restores the
/// count-based sizing on Android.
#[test]
fn desktop_and_off_ignore_the_topology() {
    for (mode, android) in [
        (TopologyMode::Auto, false),
        (TopologyMode::Off, false),
        (TopologyMode::Off, true),
    ] {
        let plan = plan(&TWO_PLUS_SIX, mode, android);
        assert_eq!(plan.shared_pool_cores(), 8, "{mode:?} android={android}");
        assert_eq!(plan.placement(), None);
        assert_eq!(plan.background_width(), None);
        assert!(plan.topology().is_some(), "the topology is still reported");
        assert_eq!(plan.mode(), mode);
    }
    let forced = plan(&TWO_PLUS_SIX, TopologyMode::On, false);
    assert_eq!(forced.shared_pool_cores(), 5);
    assert!(forced.placement().is_some());
}

/// The online list counts every online CPU, whatever mask the reading thread has.
#[test]
fn online_list_counts_ranges_and_single_cpus() {
    let root = tempfile::tempdir().unwrap();
    let online = |list: &str| {
        std::fs::write(root.path().join("online"), list).unwrap();
        online_cpus(root.path())
    };
    assert_eq!(online("0-7\n"), Some(8));
    assert_eq!(online("0-3,6-7\n"), Some(6));
    assert_eq!(online("0\n"), Some(1));
    assert_eq!(online("0,2,4-5"), Some(4));
    for malformed in ["", "\n", "0-", "3-1", "a-b", "0-3,", "0--3"] {
        assert_eq!(online(malformed), None, "{malformed:?}");
    }
    assert_eq!(online_cpus(Path::new("/nonexistent/cpu")), None);
    // The fake tree the topology tests read carries a list too.
    assert_eq!(online_cpus(capacities(&ONE_THREE_FOUR).path()), Some(8));
}

/// A phone whose first planning thread starts in a cpuset narrower than the machine still
/// plans for every online CPU, instead of undersizing every pool for the life of the process.
#[test]
fn android_counts_online_cpus_instead_of_the_startup_mask() {
    assert_eq!(machine_cores(4, Some(8)), 8);
    // An unreadable list keeps the mask's count, and the list never lowers it.
    assert_eq!(machine_cores(4, None), 4);
    assert_eq!(machine_cores(8, Some(6)), 8);
    let plan = |logical| {
        let topology = read(&capacities(&ONE_THREE_FOUR));
        CpuPlan::new(logical, Some(topology), TopologyMode::Auto, true)
    };
    let narrow = plan(4);
    assert_eq!((narrow.logical(), narrow.shared_pool_cores()), (4, 4));
    let whole = plan(machine_cores(4, Some(8)));
    assert_eq!((whole.logical(), whole.shared_pool_cores()), (8, 5));
}

#[test]
fn unreadable_topology_keeps_the_logical_count() {
    let plan = CpuPlan::new(8, None, TopologyMode::On, true);
    assert_eq!(plan.shared_pool_cores(), 8);
    assert_eq!(plan.placement(), None);
    assert_eq!(plan.background_width(), None);
    assert_eq!(plan.to_string(), "8 logical cores, topology unreadable");
}

/// The core cap still bounds pools sized by the topology.
#[test]
fn core_cap_bounds_topology_sizing() {
    let topology = read(&capacities(&[400, 400, 400, 400, 1024, 1024, 1024, 1024]));
    assert_eq!(
        CpuPlan::new(3, Some(topology), TopologyMode::Auto, true).shared_pool_cores(),
        3
    );
}

/// The startup line names the tiers and, when cores are reserved, who runs where.
#[test]
fn plans_describe_the_topology_and_placement() {
    assert_eq!(
        plan(&TWO_PLUS_SIX, TopologyMode::Auto, true).to_string(),
        "8 logical cores, cpu_capacity 1024x2[6,7] 380x6[0,1,2,3,4,5], \
         frame cpus [6, 7], background cpus [0, 1, 2, 3, 4, 5]"
    );
    assert_eq!(
        plan(&TWO_PLUS_SIX, TopologyMode::Auto, false).to_string(),
        "8 logical cores, cpu_capacity 1024x2[6,7] 380x6[0,1,2,3,4,5], topology not applied"
    );
    assert_eq!(
        plan(&[1024; 4], TopologyMode::Auto, true).to_string(),
        "4 logical cores, cpu_capacity 1024x4[0,1,2,3], no frame cpus reserved"
    );
}

/// A core cap only ever shrinks the pools; malformed or zero caps keep the machine's count.
#[test]
fn core_cap_shrinks_pools_and_ignores_invalid_values() {
    assert_eq!(capped_cores(12, None), 12);
    assert_eq!(capped_cores(12, Some("2")), 2);
    assert_eq!(capped_cores(12, Some(" 4 ")), 4);
    assert_eq!(capped_cores(4, Some("16")), 4);
    assert_eq!(capped_cores(12, Some("0")), 12);
    assert_eq!(capped_cores(12, Some("two")), 12);
}

#[test]
fn topology_override_values() {
    assert_eq!(TopologyMode::parse(None), TopologyMode::Auto);
    assert_eq!(TopologyMode::parse(Some(" OFF ")), TopologyMode::Off);
    assert_eq!(TopologyMode::parse(Some("0")), TopologyMode::Off);
    assert_eq!(TopologyMode::parse(Some("on")), TopologyMode::On);
    assert_eq!(TopologyMode::parse(Some("auto")), TopologyMode::Auto);
    assert_eq!(TopologyMode::parse(Some("maybe")), TopologyMode::Auto);
}

/// Each `CINNABAR_` override has a `debug.cinnabar.` system property for Android.
#[test]
fn android_properties_mirror_overrides() {
    assert_eq!(
        android_property(TOPOLOGY_ENV).as_deref(),
        Some("debug.cinnabar.cpu_topology")
    );
    assert_eq!(
        android_property(CORES_ENV).as_deref(),
        Some("debug.cinnabar.cpu_cores")
    );
    assert_eq!(android_property("WGPU_BACKEND"), None);
}
