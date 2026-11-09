//! Timeline keys, keyframe order and malformed animations in server-pack animation files.
use super::*;

/// The keyframe times and values of the first channel of the clip compiled for `identifier`.
fn first_channel_keyframes(
    compiled: &assets::CompiledEntityAssets,
    identifier: &str,
) -> Vec<(f32, [f32; 3])> {
    let clip = compiled
        .animation_clips
        .iter()
        .find(|clip| compiled.symbols[clip.symbol as usize].identifier.as_ref() == identifier)
        .unwrap();
    let channel = &compiled.animation_channels[clip.first_channel as usize];
    compiled.animation_keyframes[channel.first_keyframe as usize..]
        [..channel.keyframe_count as usize]
        .iter()
        .map(|keyframe| {
            (
                keyframe.time_seconds.get(),
                keyframe.value.map(EntityGeometryScalar::get),
            )
        })
        .collect()
}

#[test]
fn malformed_or_unsupported_time_update_skips_only_its_animation() {
    for authored in [
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!("query.modified_distance_moved +"),
        serde_json::json!("query.unreviewed_time"),
    ] {
        let pack = animation_pack(false);
        set_walk_time_update(pack.path(), authored.clone());
        let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
        let identifier = |symbol: u32| compiled.assets.symbols[symbol as usize].identifier.as_ref();
        let clips = compiled
            .assets
            .animation_clips
            .iter()
            .map(|clip| identifier(clip.symbol))
            .collect::<Vec<_>>();
        assert!(!clips.contains(&"animation.test.walk"), "{authored}");
        assert!(clips.contains(&"animation.test.attack"), "{authored}");
        assert!(
            compiled.reference_outcomes.iter().any(|outcome| matches!(
                outcome,
                pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback {
                    symbol,
                    reason: pack_entity_compiler::FallbackReason::InvalidDefinition,
                    ..
                } if identifier(*symbol) == "animation.test.walk"
            )),
            "{authored}"
        );
    }
}

#[test]
fn timeline_keyframes_play_in_time_order() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animations/test.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"loop":true,"animation_length":12,"bones":{"root":{"position":{"10.0":[3,0,0],"2.0":[2,0,0],"0":{"pre":[0,0,0],"post":[1,0,0]}}}}},"animation.test.attack":{"bones":{"arm":{"rotation":[0,30,0]}}}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        first_channel_keyframes(&compiled, "animation.test.walk"),
        vec![
            (0.0, [0.0, 0.0, 0.0]),
            (0.0, [1.0, 0.0, 0.0]),
            (2.0, [2.0, 0.0, 0.0]),
            (10.0, [3.0, 0.0, 0.0]),
        ]
    );
}

#[test]
fn a_channel_keyed_by_something_other_than_time_drops_only_that_channel() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animations/test.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"loop":true,"animation_length":2,"bones":{"root":{"position":{"0.0":[0,0,0],"1.0":[1,2,3]},"rotation":{"vector":[0,"-math.cos(query.life_time*180)*22",0]}}}},"animation.test.attack":{"animation_length":0.5,"bones":{"arm":{"rotation":{"0.0":[0,0,0],"0.5":[0,30,0]}}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    let assets = &compiled.assets;
    let walk = assets
        .symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "animation.test.walk")
        .unwrap() as u32;
    let walk_clips: Vec<_> = assets
        .animation_clips
        .iter()
        .filter(|clip| clip.symbol == walk)
        .collect();
    assert!(!walk_clips.is_empty());
    for clip in walk_clips {
        let channels = &assets.animation_channels
            [clip.first_channel as usize..(clip.first_channel + clip.channel_count) as usize];
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].property, EntityAnimationProperty::Translation);
    }
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback {
            symbol,
            reason: pack_entity_compiler::FallbackReason::UnsupportedOptionalExpression,
            ..
        } if *symbol == walk
    )));
}

#[test]
fn timeline_keys_that_are_not_plain_decimal_times_are_ignored() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animations/test.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"loop":true,"animation_length":2,"bones":{"root":{"position":{"0.0":[0,0,0],"0.5s":[9,9,9],"-1":[9,9,9],".5":[9,9,9],"1.":[9,9,9],"1e1":[9,9,9]," 2":[9,9,9],"1.0":[1,2,3]}}}},"animation.test.attack":{"bones":{"arm":{"rotation":[0,30,0]}}}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        first_channel_keyframes(&compiled, "animation.test.walk"),
        vec![(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 2.0, 3.0])]
    );
}

#[test]
fn invalid_animation_falls_back_without_dropping_the_stack() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animations/test.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"animation_length":"NaN","bones":{"root":{"position":[1,0,0]}}},"animation.test.attack":{"bones":{"arm":{"rotation":[0,30,0]}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    let identifier = |symbol: u32| compiled.assets.symbols[symbol as usize].identifier.as_ref();
    assert!(
        compiled
            .assets
            .animation_clips
            .iter()
            .all(|clip| identifier(clip.symbol) != "animation.test.walk")
    );
    assert_eq!(
        first_channel_keyframes(&compiled.assets, "animation.test.attack"),
        vec![(0.0, [0.0, 30.0, 0.0])]
    );
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback {
            symbol,
            reason: pack_entity_compiler::FallbackReason::InvalidDefinition,
            ..
        } if identifier(*symbol) == "animation.test.walk"
    )));
    assert_eq!(compiled.assets.rig_bindings.len(), 1);
}
