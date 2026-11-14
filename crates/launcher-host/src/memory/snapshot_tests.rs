use super::*;

const MIB: u64 = 1 << 20;

const ROLLUP: &str = "\
5c1d0000-7ffe3f1fe000 ---p 00000000 00:00 0                              [rollup]
Rss:              831488 kB
Pss:              655360 kB
Pss_Dirty:        500000 kB
Pss_Anon:         498000 kB
Pss_File:         150000 kB
Pss_Shmem:          7360 kB
Shared_Clean:     200000 kB
Private_Dirty:    498000 kB
Anonymous:        498000 kB
Swap:              12288 kB
SwapPss:           11000 kB
Locked:                0 kB
";

const MEMINFO: &str = "\
MemTotal:        5493760 kB
MemFree:          102400 kB
MemAvailable:    1239040 kB
Buffers:            4096 kB
SwapCached:        20480 kB
SwapTotal:       4194304 kB
SwapFree:        2097152 kB
";

#[test]
fn the_rollup_gives_resident_proportional_and_swapped_bytes() {
    let memory = ProcessMemory::from_smaps_rollup(ROLLUP);
    assert_eq!(
        memory,
        ProcessMemory {
            resident: Some(831_488 * 1024),
            proportional: Some(655_360 * 1024),
            swapped: Some(12_288 * 1024),
        }
    );
    assert!(!memory.is_empty());
}

#[test]
fn similarly_named_rollup_fields_are_not_mistaken_for_the_totals() {
    let memory = ProcessMemory::from_smaps_rollup("Pss_Anon: 5 kB\nSwapPss: 7 kB\nRss_x: 1 kB\n");
    assert!(memory.is_empty());
}

#[test]
fn an_unreadable_rollup_falls_back_to_the_resident_pages_of_statm() {
    let memory =
        ProcessMemory::from_smaps_rollup("").or_statm("250000 20480 3000 12 0 9000 0\n", 4096);
    assert_eq!(memory.resident, Some(20_480 * 4096));
    assert_eq!(memory.proportional, None);
    assert_eq!(memory.swapped, None);
}

#[test]
fn a_rollup_resident_size_is_kept_over_statm() {
    let memory = ProcessMemory::from_smaps_rollup(ROLLUP).or_statm("1 2 3\n", 4096);
    assert_eq!(memory.resident, Some(831_488 * 1024));
}

#[test]
fn malformed_statm_gives_no_resident_size() {
    assert_eq!(statm_resident_bytes("", 4096), None);
    assert_eq!(statm_resident_bytes("250000", 4096), None);
    assert_eq!(statm_resident_bytes("250000 many", 4096), None);
    assert_eq!(statm_resident_bytes("1 18446744073709551615", 4096), None);
    assert_eq!(statm_resident_bytes("1 3", 16_384), Some(3 * 16_384));
}

#[test]
fn meminfo_gives_the_device_totals() {
    assert_eq!(
        SystemMemory::from_meminfo(MEMINFO),
        SystemMemory {
            total: Some(5_493_760 * 1024),
            available: Some(1_239_040 * 1024),
            swap_total: Some(4_194_304 * 1024),
            swap_free: Some(2_097_152 * 1024),
        }
    );
    assert_eq!(
        SystemMemory::from_meminfo("MemTotal: lots kB\nMemFree: 1 kB\n"),
        SystemMemory::default()
    );
}

#[test]
fn a_full_snapshot_is_one_line_in_whole_mebibytes() {
    let snapshot = Snapshot {
        process: ProcessMemory::from_smaps_rollup(ROLLUP),
        children: vec![
            ChildMemory {
                name: "bedrock_core".to_owned(),
                pid: 71,
                memory: ProcessMemory {
                    resident: Some(38 * MIB),
                    proportional: Some(30 * MIB),
                    swapped: Some(0),
                },
            },
            ChildMemory {
                name: "bedrock server".to_owned(),
                pid: 90,
                memory: ProcessMemory {
                    resident: Some(911 * MIB),
                    ..ProcessMemory::default()
                },
            },
        ],
        system: SystemMemory::from_meminfo(MEMINFO),
        platform: Some(PlatformMemory {
            available: 1180 * MIB,
            threshold: 216 * MIB,
            low: false,
        }),
        counters: Counters {
            pools: vec![
                ("gpu_allocated", 310 * MIB),
                ("chunk_arena", 48 * MIB + 600_000),
            ],
            counts: vec![("sections", 1234)],
        },
    };
    assert_eq!(
        snapshot.line("world_entry"),
        "memory event=world_entry rss=812M pss=640M swap=12M \
         children=[bedrock_core:71 rss=38M pss=30M swap=0M; bedrock_server:90 rss=911M] \
         system=[available=1210M total=5365M swap_free=2048M swap_total=4096M] \
         android=[available=1180M threshold=216M low=false] \
         pools=[gpu_allocated=310M chunk_arena=49M] counts=[sections=1234]"
    );
}

#[test]
fn unknown_figures_and_empty_groups_are_left_out() {
    assert_eq!(Snapshot::default().line("startup"), "memory event=startup");
    let snapshot = Snapshot {
        process: ProcessMemory {
            resident: Some(5 * MIB),
            ..ProcessMemory::default()
        },
        system: SystemMemory {
            total: Some(8 * 1024 * MIB),
            ..SystemMemory::default()
        },
        ..Snapshot::default()
    };
    assert_eq!(
        snapshot.line("menu ready"),
        "memory event=menu_ready rss=5M system=[total=8192M]"
    );
}

#[test]
fn every_line_starts_with_the_prefix_reports_search_for() {
    assert!(
        Snapshot::default()
            .line("background")
            .starts_with(LINE_PREFIX)
    );
}

#[test]
fn a_process_name_cannot_break_out_of_its_group() {
    let snapshot = Snapshot {
        children: vec![ChildMemory {
            name: "a]; b=[c:1".to_owned(),
            pid: 2,
            memory: ProcessMemory::default(),
        }],
        ..Snapshot::default()
    };
    assert_eq!(snapshot.line("x"), "memory event=x children=[a___b__c_1:2]");
}

#[test]
fn sizes_round_to_the_nearest_mebibyte() {
    assert_eq!(mib(0), "0M");
    assert_eq!(mib(MIB / 2 - 1), "0M");
    assert_eq!(mib(MIB / 2), "1M");
    assert_eq!(mib(u64::MAX), "17592186044415M");
}

#[test]
fn the_android_packaging_manifest_names_the_same_line_prefix() {
    #[derive(serde::Deserialize)]
    struct Runtime {
        memory_line: String,
    }
    let runtime: Runtime =
        serde_json::from_str(include_str!("../../../../packaging/android/runtime.json"))
            .expect("the tracked Android runtime manifest parses");
    assert_eq!(runtime.memory_line, LINE_PREFIX);
}
