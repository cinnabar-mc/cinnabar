use super::*;

/// A guest whose frame passes a list of identifiers to `terrain.find-blocks`.
const FIND_BLOCKS: &str = r#"(component
  (import "$TERRAIN" (instance $terrain
    (type $p (record (field "x" s32) (field "y" s32) (field "z" s32)))
    (export "position" (type $position (eq $p)))
    (type $e (enum "denied" "unavailable" "invalid-query" "out-of-range" "budget-exhausted" "limit-exceeded"))
    (export "read-error" (type $read-error (eq $e)))
    (export "find-blocks" (func (param "identifiers" (list string)) (param "range" f32) (param "limit" u32)
      (result (result (list $position) (error $read-error)))))))
  (alias export $terrain "find-blocks" (func $find))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 32768))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $find) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "find" (func $find (param i32 i32 f32 i32 i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame") $FRAME))
  (core instance $host (export "find" (func $lower)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))"#;

#[test]
fn guest_lists_share_a_copy_budget_before_capability_checks() {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let count = 32;
    let length = crate::execution::HOSTCALL_BYTES / count;
    assert!(4096 + length <= 65536);
    let mut frame = String::new();
    for index in 0..count {
        let descriptor = 2048 + index * 8;
        frame.push_str(&format!(
            "i32.const {descriptor} i32.const 4096 i32.store \
             i32.const {} i32.const {length} i32.store ",
            descriptor + 4
        ));
    }
    frame.push_str(&format!(
        "i32.const 2048 i32.const {count} f32.const 64 i32.const 16 i32.const 512 call $find"
    ));
    let source = FIND_BLOCKS
        .replace("$TERRAIN", &format!("{name}/terrain@{version}"))
        .replace("$FRAME", &frame);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bounded.wat");
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load(&path).unwrap();
    let error = host.frame(false).unwrap_err();
    assert!(format!("{error:#}").contains("hostcalls"), "{error:#}");
    assert!(!host.is_active());
}

#[test]
fn guest_memory_ranges_are_checked_before_hud_output() {
    for frame in [
        "i32.const 65535 i32.const 5 i32.const 128 call $label",
        "i32.const -1 i32.const 2 i32.const 128 call $label",
        "i32.const 0 i32.const -1 i32.const 128 call $label",
    ] {
        let (_directory, mut host) = load_fixture(frame);
        assert!(host.frame(false).is_err());
        assert!(!host.is_active());
        assert!(host.label().is_none());
    }
}

#[test]
fn wasi_authority_is_unlinked() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unlinked.wat");
    for interface in [
        "wasi:filesystem/types@0.2.0",
        "wasi:cli/environment@0.2.0",
        "wasi:sockets/network@0.2.0",
        "wasi:clocks/wall-clock@0.2.0",
        "wasi:random/random@0.2.0",
    ] {
        let source = fixture("", "Hello").replacen(
            "(component",
            &format!("(component (import \"{interface}\" (instance (export \"probe\" (func))))"),
            1,
        );
        std::fs::write(&path, source).unwrap();
        // The packet-mod grants never link WASI into an ordinary mod.
        let trusted = ModGrants {
            packets: true,
            wasi: true,
            ..Default::default()
        };
        for grants in [ModGrants::default(), trusted] {
            let error = ModHost::load_with_grants(&path, grants)
                .err()
                .expect("authority must be unlinked");
            assert!(format!("{error:#}").contains(interface), "{error:#}");
        }
    }
}

#[test]
#[cfg(unix)]
fn settings_link_is_refused_before_guest_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = directory.path().join("mod.wat");
    let target = outside.path().join("preferences.json");
    std::fs::write(&path, fixture("", "Hello")).unwrap();
    std::fs::write(&target, "{\"value\":1}").unwrap();
    std::os::unix::fs::symlink(&target, path.with_extension("settings.json")).unwrap();
    let error = ModHost::load_with_grants(
        &path,
        ModGrants {
            settings: true,
            ..Default::default()
        },
    )
    .err()
    .expect("linked settings must fail admission");
    assert!(
        format!("{error:#}").contains("read mod settings"),
        "{error:#}"
    );
    assert_eq!(std::fs::read_to_string(target).unwrap(), "{\"value\":1}");
    assert!(
        ModHost::load(&path).is_ok(),
        "denied settings are never read"
    );
}
