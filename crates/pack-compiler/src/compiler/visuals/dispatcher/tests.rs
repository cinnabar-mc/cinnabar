use super::*;
use assets::RegistryProvenance;

#[test]
fn resolution_pins_the_unique_pinned_registry_air_identity() {
    let records = assets::read_registry(include_bytes!(
        "../../../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode pinned registry");
    let identity = resolve_canonical_air(&records).expect("unique canonical air");

    // The pinned protocol-1001 registry must keep resolving today's
    // exact identity so default v1001 compilation stays byte-identical.
    assert_eq!(identity.sequential_id, 13_094);
    assert_eq!(identity.network_hash, 0xdbf4_4120);

    let air = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:air")
        .expect("canonical air");
    assert!(is_canonical_air(air, identity));

    let mut custom = air.clone();
    custom.name = "custom:air".into();
    assert!(!is_canonical_air(&custom, identity));

    let mut wrong_state = air.clone();
    wrong_state.canonical_state = r#"{"custom":true}"#.into();
    assert!(!is_canonical_air(&wrong_state, identity));

    let mut wrong_hash = air.clone();
    wrong_hash.network_hash ^= 1;
    assert!(!is_canonical_air(&wrong_hash, identity));

    let mut wrong_id = air.clone();
    wrong_id.sequential_id -= 1;
    assert!(!is_canonical_air(&wrong_id, identity));
}

fn synthetic_record(name: &str, state: &str, id: u32, hash: u32) -> RegistryRecord {
    RegistryRecord {
        beacon_tint: assets::BeaconTint::Unresolved,
        traits: assets::registry::BlockTraits::empty(),
        survival: assets::registry::SurvivalRule::None,
        properties: Box::new([]),
        sequential_id: id,
        network_hash: hash,
        name: name.into(),
        canonical_state: state.into(),
        flags: BlockFlags::AIR,
        model_family: ModelFamily::Air,
        contributor_role: ContributorRole::Air,
        model_state: Default::default(),
        face_coverage: 0,
        collision_seed: Default::default(),
        provenance: RegistryProvenance::PMMP,
    }
}

#[test]
fn resolution_fails_closed_with_distinct_errors_for_zero_multiple_and_decoy_air() {
    let zero = [synthetic_record("minecraft:stone", "{}", 1, 0x0000_0001)];
    let multiple = [
        synthetic_record("minecraft:air", "{}", 10, 0x0000_000a),
        synthetic_record("minecraft:air", "{}", 11, 0x0000_000b),
    ];
    let decoy = [synthetic_record(
        "minecraft:air",
        r#"{"legacy":true}"#,
        12,
        0x0000_000c,
    )];

    let zero_error = resolve_canonical_air(&zero).expect_err("no canonical air record");
    let multiple_error =
        resolve_canonical_air(&multiple).expect_err("conflicting canonical air records");
    let decoy_error = resolve_canonical_air(&decoy).expect_err("decoy canonical air name");

    for error in [&zero_error, &multiple_error, &decoy_error] {
        assert!(matches!(error, AssetError::InvalidCompiledAssets { .. }));
    }
    assert_ne!(zero_error.to_string(), multiple_error.to_string());
    assert_ne!(multiple_error.to_string(), decoy_error.to_string());
    assert_ne!(zero_error.to_string(), decoy_error.to_string());
}

#[test]
fn resolution_rejects_a_decoy_even_after_a_valid_candidate() {
    let records = [
        synthetic_record("minecraft:air", "{}", 20, 0x0000_0014),
        synthetic_record("minecraft:air", r#"{"legacy":true}"#, 21, 0x0000_0015),
    ];
    let error = resolve_canonical_air(&records).expect_err("decoy after valid candidate");
    assert!(
        error.to_string().contains("fails the canonical-air"),
        "unexpected error: {error}"
    );
}
