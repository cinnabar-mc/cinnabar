use super::*;

fn query(x: i32) -> Query {
    Query {
        dimension: 0,
        min: [x, 64, 0],
        size: [1; 3],
        load: false,
    }
}

fn frame(bridge: &Bridge, id: u32, sequence: u64, region: &Region) -> Value {
    json!({"version": VERSION, "session": bridge.session.to_string(), "nonce": bridge.nonce,
        "id": id, "sequence": sequence, "part": 0, "total": 1,
        "data": json!({"error": false, "region": region}).to_string()})
}

fn observe(bridge: &Bridge, value: &Value) {
    bridge.observe(&format!("[Scripting][warning]- {PREFIX}{value}"));
}

#[test]
fn rejects_wrong_launch_stale_frames_and_invalid_cells() {
    let bridge = Bridge::new().unwrap();
    let source = bridge.source().unwrap();
    let query = query(0);
    assert!(source.read(&query).unwrap().is_none());
    let region = Region {
        session: bridge.session,
        revision: 0,
        palette: vec![],
        cells: vec![UNKNOWN],
    };
    let valid = frame(&bridge, 1, 1, &region);
    for key in ["nonce", "session"] {
        let mut wrong = valid.clone();
        wrong[key] = json!("wrong");
        observe(&bridge, &wrong);
        assert!(lock(&bridge.state).subscriptions[0].region.is_none());
    }
    let mut wrong_version = valid.clone();
    wrong_version["version"] = json!(VERSION + 1);
    observe(&bridge, &wrong_version);
    let mut oversized = valid.clone();
    oversized["total"] = json!(MAX_RESPONSE_BYTES + 1);
    observe(&bridge, &oversized);
    assert!(lock(&bridge.state).subscriptions[0].region.is_none());
    observe(&bridge, &valid);
    let first = source.read(&query).unwrap().unwrap();
    assert_eq!(first.revision, 1);
    let invalid = Region {
        cells: vec![0],
        ..region.clone()
    };
    observe(&bridge, &frame(&bridge, 1, 2, &invalid));
    assert!(Arc::ptr_eq(&first, &source.read(&query).unwrap().unwrap()));
    observe(&bridge, &frame(&bridge, 1, 3, &region));
    assert!(Arc::ptr_eq(&first, &source.read(&query).unwrap().unwrap()));
    let changed = Region {
        palette: vec![mod_api::local_terrain::BlockState {
            name: "minecraft:stone".into(),
            states: "{}".into(),
        }],
        cells: vec![0],
        ..region.clone()
    };
    observe(&bridge, &frame(&bridge, 1, 4, &changed));
    assert_eq!(source.read(&query).unwrap().unwrap().revision, 2);
    observe(&bridge, &valid);
    assert_eq!(source.read(&query).unwrap().unwrap().revision, 2);
    let oversized = "x".repeat(MAX_FRAME_BYTES + 1);
    bridge.observe(&format!("{PREFIX}{oversized}"));
    assert_eq!(source.read(&query).unwrap().unwrap().revision, 2);
}

#[test]
fn fragments_are_ordered_bounded_and_cannot_replace_newer_assembly() {
    let bridge = Bridge::new().unwrap();
    let source = bridge.source().unwrap();
    let q = query(0);
    source.read(&q).unwrap();
    let region = Region {
        session: bridge.session,
        revision: 0,
        palette: vec![],
        cells: vec![UNKNOWN],
    };
    let mut first = frame(&bridge, 1, 2, &region);
    let data = first["data"].as_str().unwrap().to_owned();
    first["data"] = json!(&data[..10]);
    first["total"] = json!(2);
    let mut second = first.clone();
    second["part"] = json!(1);
    second["data"] = json!(&data[10..]);
    observe(&bridge, &second);
    assert!(lock(&bridge.state).subscriptions[0].region.is_none());
    observe(&bridge, &first);
    let mut stale = first.clone();
    stale["sequence"] = json!(1);
    observe(&bridge, &stale);
    observe(&bridge, &second);
    assert_eq!(source.read(&q).unwrap().unwrap().revision, 1);
}

#[test]
fn stamps_cannot_collide_between_distinct_region_queries() {
    let bridge = Bridge::new().unwrap();
    let source = bridge.source().unwrap();
    let a = query(0);
    let b = query(1);
    source.read(&a).unwrap();
    source.read(&b).unwrap();
    let region = Region {
        session: bridge.session,
        revision: 0,
        palette: vec![],
        cells: vec![UNKNOWN],
    };
    observe(&bridge, &frame(&bridge, 1, 1, &region));
    observe(&bridge, &frame(&bridge, 2, 1, &region));
    assert_ne!(
        source.read(&a).unwrap().unwrap().revision,
        source.read(&b).unwrap().unwrap().revision
    );
}

#[test]
fn subscriptions_expire_and_reader_is_reused_without_process_ownership() {
    let bridge = Bridge::new().unwrap();
    let source = bridge.source().unwrap();
    let second = bridge.source().unwrap();
    assert!(Arc::ptr_eq(&source, &second));
    for x in 0..MAX_QUERIES {
        source.read(&query(x as i32)).unwrap();
    }
    assert_eq!(
        source.read(&query(MAX_QUERIES as i32)),
        Err(Error::BudgetExhausted)
    );
    let later = Instant::now() + TTL;
    assert!(bridge.snapshot(later).is_empty());
    source.read(&query(0)).unwrap();
    drop(source);
    drop(second);
    assert!(bridge.snapshot(Instant::now()).is_empty());
    let source = bridge.source().unwrap();
    source.read(&query(1)).unwrap();
    bridge.stop();
    assert_eq!(source.read(&query(1)), Err(Error::Unavailable));
    assert!(bridge.source().is_none());
}

#[test]
fn pack_merge_preserves_other_references_and_rejects_malformed_lists() {
    let other = json!({"pack_id": MODULE_ID, "version": [4, 5, 6], "custom": {"kept": true}});
    let merged =
        merge_packs(json!([other.clone(), {"pack_id": PACK_ID, "version": [0, 1, 0]}])).unwrap();
    assert_eq!(merged[0], other);
    assert_eq!(merged.as_array().unwrap().len(), 2);
    assert_eq!(merged[1]["version"], json!(PACK_VERSION));
    assert_eq!(merge_packs(merged.clone()).unwrap(), merged);
    for bad in [
        json!({}),
        json!([null]),
        json!([{"pack_id": MODULE_ID, "version": [1]}]),
    ] {
        assert!(merge_packs(bad).is_err());
    }
}

#[test]
fn script_permutations_are_converted_to_native_typed_states() {
    let bridge = Bridge::new().unwrap();
    let source = bridge.source().unwrap();
    let q = query(0);
    source.read(&q).unwrap();
    let region = Region {
        session: bridge.session,
        revision: 0,
        cells: vec![0],
        palette: vec![mod_api::local_terrain::BlockState {
            name: "minecraft:oak_stairs".into(),
            states: r#"{"upside_down_bit":true,"weirdo_direction":2}"#.into(),
        }],
    };
    observe(&bridge, &frame(&bridge, 1, 1, &region));
    let sampled = source.read(&q).unwrap().unwrap();
    let states: Value = serde_json::from_str(&sampled.palette[0].states).unwrap();
    assert_eq!(states["upside_down_bit"], json!({"type":"byte","value":1}));
    assert_eq!(states["weirdo_direction"], json!({"type":"int","value":2}));
}

#[cfg(windows)]
#[test]
fn native_bds_companion_reads_updates_and_releases_private_world() {
    let Some(binary) = std::env::var_os("CINNABAR_BDS_TEST_BIN").map(std::path::PathBuf::from)
    else {
        eprintln!("SKIP native BDS terrain fixture: CINNABAR_BDS_TEST_BIN is not set");
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    fn link_tree(source: &Path, target: &Path) {
        fs::create_dir_all(target).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let dest = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                link_tree(&entry.path(), &dest);
            } else {
                fs::hard_link(entry.path(), dest).unwrap();
            }
        }
    }
    fs::hard_link(&binary, scratch.path().join("bedrock_server.exe")).unwrap();
    for name in [
        "definitions",
        "data",
        "behavior_packs",
        "resource_packs",
        "treatment_tags",
    ] {
        let source = binary.parent().unwrap().join(name);
        if source.is_dir() {
            link_tree(&source, &scratch.path().join(name));
        }
    }
    let config = scratch.path().join("config/default");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("permissions.json"),
        r#"{"allowed_modules":["@minecraft/server"]}"#,
    )
    .unwrap();
    fs::write(scratch.path().join("allowlist.json"), "[]").unwrap();
    let world_dir = scratch.path().join("worlds/probe");
    fs::create_dir_all(&world_dir).unwrap();
    let bridge = install(&world_dir).unwrap();
    // Windows may reserve different ephemeral ranges for TCP and UDP.
    let port = (0..32)
        .find_map(|_| {
            let candidate = super::super::ports::free_loopback_port().ok()?;
            super::super::ports::free_bds_port(1, candidate).ok()
        })
        .expect("available loopback TCP and UDP fixture port");
    fs::write(scratch.path().join("server.properties"), format!(
        "server-name=Terrain test\nserver-ip=127.0.0.1\nlevel-name=probe\nlevel-type=FLAT\nlevel-seed=12345\ngamemode=creative\ndifficulty=peaceful\nallow-cheats=true\nonline-mode=false\nenable-lan-visibility=false\ntransport=nethernet\nserver-port={port}\nserver-udp-ports=127.0.0.1:{port}-{port}:{port}-{port}\nmax-players=1\nview-distance=5\ncontent-log-console-output-enabled=true\n"
    )).unwrap();
    let mut command = std::process::Command::new(scratch.path().join("bedrock_server.exe"));
    command.current_dir(scratch.path());
    let (_keep, cancel) = super::super::cancel_pair();
    let process = super::super::process::launch_with_terrain(
        command,
        super::super::process::Launch {
            address: format!("127.0.0.1:{port}"),
            timeout: Duration::from_secs(90),
            can_pause: false,
            ready: |line| line.contains("Server started."),
            label: "native terrain fixture".into(),
            on_exit: None,
        },
        &cancel,
        Some(bridge.clone()),
    )
    .unwrap();
    struct Stop(Arc<Process>);
    impl Drop for Stop {
        fn drop(&mut self) {
            use super::super::process::Instance as _;
            let _ = self.0.stop(Duration::from_secs(20));
        }
    }
    let _stop = Stop(process.clone());
    let source = bridge.source().unwrap();
    let mut q = Query {
        dimension: 1,
        min: [1024, 64, 1024],
        size: [MAX_EXTENT; 3],
        load: false,
    };
    let sample = |q: &Query| source.read(q).ok().flatten();
    test_time::eventually_within(
        Duration::from_secs(30),
        "unloaded destination remains unknown",
        || sample(&q).is_some_and(|region| region.cells.iter().all(|cell| *cell == UNKNOWN)),
    );
    q.load = true;
    test_time::eventually_within(
        Duration::from_secs(30),
        "native destination loading",
        || sample(&q).is_some_and(|region| region.cells.iter().all(|cell| *cell != UNKNOWN)),
    );
    process.send("execute in nether run setblock 1024 64 1024 oak_stairs [\"upside_down_bit\"=true,\"weirdo_direction\"=2]").unwrap();
    test_time::eventually_within(
        Duration::from_secs(30),
        "real block update and typed permutation",
        || {
            sample(&q).is_some_and(|region| {
                let Some(block) = region.palette.get(region.cells[0] as usize) else {
                    return false;
                };
                let states: Value = serde_json::from_str(&block.states).unwrap();
                block.name == "minecraft:oak_stairs"
                    && states["upside_down_bit"]["value"] == 1
                    && states["weirdo_direction"]["value"] == 2
            })
        },
    );
    drop(source);
    assert!(bridge.snapshot(Instant::now()).is_empty());
    println!(
        "PASS native BDS world-local companion: unknown cells, chunk loading, block updates, typed states and reader release"
    );
}
