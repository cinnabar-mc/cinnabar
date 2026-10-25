use super::*;
use crate::manifest::{in_namespace, world_namespace};
use std::collections::BTreeSet;

fn spec(depth: WorldDepth, textures: &[&str]) -> WorldMaterialSpec {
    WorldMaterialSpec {
        source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }".into(),
        textures: textures.iter().map(|path| (*path).to_owned()).collect(),
        params: vec![0.5],
        depth,
        blend: WorldBlend::Opaque,
        effects: Default::default(),
    }
}

fn authority<'a>(
    permissions: &'a BTreeSet<Permission>,
    assets: &'a BTreeSet<String>,
) -> WorldAuthority<'a> {
    WorldAuthority {
        permissions,
        namespace: Some("example"),
        assets,
    }
}

#[test]
fn namespaces_are_well_formed_and_never_reserved() {
    assert!(world_namespace("example"));
    assert!(world_namespace("my-server.v2"));
    for rejected in ["", "minecraft", "Example", "a:b", &"n".repeat(65)] {
        assert!(!world_namespace(rejected), "{rejected}");
    }
    assert!(in_namespace("example:moth", "example"));
    for outside in [
        "example:",
        "minecraft:pig",
        "examples:moth",
        "example:moth:extra",
        "example",
    ] {
        assert!(!in_namespace(outside, "example"), "{outside}");
    }
}

#[test]
fn materials_name_only_signed_textures_and_need_through_wall_authority_for_always() {
    let granted = BTreeSet::from([Permission::RenderWorldMaterial]);
    let assets = BTreeSet::from(["textures/glow.png".to_owned()]);
    let authority = authority(&granted, &assets);
    assert!(
        authority
            .validate_material(&spec(WorldDepth::Test, &["textures/glow.png"]))
            .is_ok()
    );
    for undeclared in ["textures/other.png", "ui/glow.png", "textures/glow.json"] {
        assert!(
            authority
                .validate_material(&spec(WorldDepth::Test, &[undeclared]))
                .is_err(),
            "{undeclared}"
        );
    }
    let mut nonfinite = spec(WorldDepth::Test, &[]);
    nonfinite.params = vec![f32::NAN];
    assert!(authority.validate_material(&nonfinite).is_err());
    let mut many = spec(WorldDepth::Test, &[]);
    many.params = vec![0.0; MAX_WORLD_MATERIAL_PARAMS + 1];
    assert!(authority.validate_material(&many).is_err());
    assert!(
        authority
            .validate_material(&spec(WorldDepth::Always, &[]))
            .is_err(),
        "depth: always needs render_through_walls"
    );
    let through = BTreeSet::from([
        Permission::RenderWorldMaterial,
        Permission::RenderThroughWalls,
    ]);
    assert!(
        super::WorldAuthority {
            permissions: &through,
            namespace: Some("example"),
            assets: &assets,
        }
        .validate_material(&spec(WorldDepth::Always, &[]))
        .is_ok()
    );
    let denied = BTreeSet::new();
    assert!(
        self::authority(&denied, &assets)
            .validate_material(&spec(WorldDepth::Test, &[]))
            .is_err()
    );
}

#[test]
fn bindings_stay_in_the_signed_namespace() {
    let granted = BTreeSet::from([Permission::RenderWorldMaterial]);
    let assets = BTreeSet::new();
    let authority = authority(&granted, &assets);
    let binding = |target| WorldBinding {
        target,
        material: 1,
    };
    assert!(
        authority
            .validate_binding(&binding(WorldTarget::Block("example:lamp".into())))
            .is_ok()
    );
    for target in [
        WorldTarget::Entity("minecraft:player".into()),
        WorldTarget::Block("minecraft:stone".into()),
        WorldTarget::Entity("other:moth".into()),
    ] {
        assert!(authority.validate_binding(&binding(target)).is_err());
    }
    let unsigned = WorldAuthority {
        namespace: None,
        ..authority
    };
    assert!(
        unsigned
            .validate_binding(&binding(WorldTarget::Block("example:lamp".into())))
            .is_err()
    );
}

#[test]
fn the_override_permission_binds_any_identifier_but_keeps_the_through_wall_rule() {
    let overriding = BTreeSet::from([
        Permission::RenderWorldMaterial,
        Permission::RenderWorldMaterialOverride,
    ]);
    let assets = BTreeSet::new();
    let authority = authority(&overriding, &assets);
    let binding = |target| WorldBinding {
        target,
        material: 1,
    };
    for target in [
        WorldTarget::Entity("minecraft:pig".into()),
        WorldTarget::Block("minecraft:stone".into()),
        WorldTarget::Entity("other:moth".into()),
    ] {
        authority.validate_binding(&binding(target)).unwrap();
    }
    for malformed in ["pig", "Minecraft:pig", ""] {
        assert!(
            authority
                .validate_binding(&binding(WorldTarget::Entity(malformed.into())))
                .is_err(),
            "{malformed:?}"
        );
    }
    assert!(
        authority
            .validate_material(&spec(WorldDepth::Always, &[]))
            .is_err(),
        "depth: always still needs render_through_walls"
    );
    // The override alone grants nothing.
    let alone = BTreeSet::from([Permission::RenderWorldMaterialOverride]);
    assert!(
        self::authority(&alone, &assets)
            .validate_binding(&binding(WorldTarget::Entity("minecraft:pig".into())))
            .is_err()
    );
}

#[test]
fn the_developer_client_denies_each_world_material_extension_unless_opted_in() {
    use crate::manifest::{WORLD_MATERIAL_EXTENSIONS, developer_permissions_with};
    let none = developer_permissions_with(|_| false);
    for extension in WORLD_MATERIAL_EXTENSIONS {
        assert!(!none.contains(&extension.permission), "{}", extension.name);
        let one = developer_permissions_with(|offered| offered.name == extension.name);
        assert!(one.contains(&extension.permission));
        assert_eq!(
            one.len(),
            none.len() + 1,
            "{} opts in alone",
            extension.name
        );
        assert!(!crate::manifest::implemented_permissions().contains(&extension.permission));
        // The table names each permission as its manifest does.
        assert_eq!(
            serde_json::to_value(extension.permission).unwrap(),
            serde_json::Value::String(extension.name.into())
        );
        assert!(extension.env.starts_with("CINNABAR_DEV_WORLD_MATERIAL_"));
    }
    // Every render_world_material_* permission is one of them.
    let extensions: Vec<_> = WORLD_MATERIAL_EXTENSIONS.iter().map(|e| e.name).collect();
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../schema/extension.json")).unwrap();
    let names = schema.to_string();
    let listed = names
        .match_indices("\"render_world_material_")
        .map(|(at, _)| names[at + 1..].split('"').next().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        listed,
        extensions
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>()
    );
}

#[test]
fn bindings_need_a_held_material_which_cannot_be_removed_while_bound() {
    let mut world = WorldMaterials::default();
    let binding = WorldBinding {
        target: WorldTarget::Entity("example:moth".into()),
        material: 1,
    };
    assert!(world.set_binding(0, Some(&binding)).is_err());
    world
        .set_material(1, Some(&spec(WorldDepth::Test, &[])))
        .unwrap();
    world.set_binding(0, Some(&binding)).unwrap();
    assert!(
        world.set_binding(1, Some(&binding)).is_err(),
        "one binding per target"
    );
    assert!(world.set_material(1, None).is_err());
    world.set_binding(0, None).unwrap();
    world.set_material(1, None).unwrap();
    assert_eq!(world, WorldMaterials::default());
}

#[test]
fn only_the_memory_limit_bounds_how_many_declarations_a_bundle_holds() {
    let mut world = WorldMaterials::default();
    // Far more materials and bindings than the former fixed caps of eight and sixty-four.
    for id in 0..200 {
        world
            .set_material(id, Some(&spec(WorldDepth::Test, &[])))
            .unwrap();
        world
            .set_binding(
                id,
                Some(&WorldBinding {
                    target: WorldTarget::Entity(format!("example:moth_{id}")),
                    material: id,
                }),
            )
            .unwrap();
    }
    let charge = world.charge();
    world.check(charge).unwrap();
    assert!(world.check(charge - 1).is_err());
}

#[test]
fn a_refused_change_leaves_the_declarations_and_their_charge_unchanged() {
    let mut world = WorldMaterials::default();
    world
        .set_material(1, Some(&spec(WorldDepth::Test, &[])))
        .unwrap();
    let binding = |name: &str| WorldBinding {
        target: WorldTarget::Block(format!("example:{name}")),
        material: 1,
    };
    world.set_binding(0, Some(&binding("lamp"))).unwrap();
    let held = world.clone();
    let charge = world.charge();
    assert!(
        world
            .set_binding_within(1, Some(&binding("post")), charge)
            .is_err(),
        "the second binding does not fit"
    );
    assert!(world.set_binding(1, Some(&binding("lamp"))).is_err());
    assert_eq!(world, held);
    // Rebinding a target to the binding that holds it, and moving a binding, keep the index.
    world.set_binding(0, Some(&binding("lamp"))).unwrap();
    world.set_binding(0, Some(&binding("post"))).unwrap();
    world.set_binding(1, Some(&binding("lamp"))).unwrap();
    world.set_binding(0, None).unwrap();
    world.set_binding(1, None).unwrap();
    world.set_material(1, None).unwrap();
    assert_eq!(world, WorldMaterials::default());
    assert_eq!(world.charge(), 0);
}

/// A blended material with `effects`.
fn with_effects(effects: WorldEffects) -> WorldMaterialSpec {
    WorldMaterialSpec {
        blend: WorldBlend::Premultiplied,
        effects,
        ..spec(WorldDepth::Test, &[])
    }
}

/// Every effect part, each within the host's ranges.
fn every_effect() -> WorldEffects {
    WorldEffects {
        displacement_blocks: Some(0.5),
        scene: true,
        outline: Some(WorldOutline { width_texels: 1.0 }),
        bloom: Some(WorldBloom {
            strength: 1.5,
            radius_pixels: 6.0,
        }),
        particles: Some(WorldParticles {
            count: 256,
            radius_blocks: 2.0,
        }),
        cells: None,
    }
}

#[test]
fn each_effect_needs_its_own_permission() {
    let assets = BTreeSet::new();
    let base = BTreeSet::from([Permission::RenderWorldMaterial]);
    let parts = [
        (
            WorldEffects {
                displacement_blocks: Some(1.0),
                ..WorldEffects::default()
            },
            Permission::RenderWorldMaterialVertex,
        ),
        (
            WorldEffects {
                scene: true,
                ..WorldEffects::default()
            },
            Permission::RenderWorldMaterialScene,
        ),
        (
            WorldEffects {
                outline: every_effect().outline,
                ..WorldEffects::default()
            },
            Permission::RenderWorldMaterialPasses,
        ),
        (
            WorldEffects {
                bloom: every_effect().bloom,
                ..WorldEffects::default()
            },
            Permission::RenderWorldMaterialPasses,
        ),
        (
            WorldEffects {
                particles: every_effect().particles,
                ..WorldEffects::default()
            },
            Permission::RenderWorldMaterialPasses,
        ),
    ];
    for (effects, permission) in parts {
        let material = with_effects(effects);
        assert!(
            authority(&base, &assets)
                .validate_material(&material)
                .is_err(),
            "{effects:?} is denied without {permission:?}"
        );
        let mut granted = base.clone();
        granted.insert(permission);
        authority(&granted, &assets)
            .validate_material(&material)
            .unwrap();
        // The other two permissions never stand in for it.
        let mut others: BTreeSet<_> = [
            Permission::RenderWorldMaterialVertex,
            Permission::RenderWorldMaterialScene,
            Permission::RenderWorldMaterialPasses,
        ]
        .into_iter()
        .filter(|other| *other != permission)
        .collect();
        others.insert(Permission::RenderWorldMaterial);
        assert!(
            authority(&others, &assets)
                .validate_material(&material)
                .is_err()
        );
    }
}

#[test]
fn effects_stay_within_the_host_ranges_and_scene_reads_only_blend() {
    let assets = BTreeSet::new();
    let all = BTreeSet::from([
        Permission::RenderWorldMaterial,
        Permission::RenderWorldMaterialVertex,
        Permission::RenderWorldMaterialScene,
        Permission::RenderWorldMaterialPasses,
    ]);
    let authority = authority(&all, &assets);
    authority
        .validate_material(&with_effects(every_effect()))
        .unwrap();
    let bad = [
        WorldEffects {
            displacement_blocks: Some(MAX_DISPLACEMENT_BLOCKS * 2.0),
            ..every_effect()
        },
        WorldEffects {
            displacement_blocks: Some(f32::NAN),
            ..every_effect()
        },
        WorldEffects {
            outline: Some(WorldOutline {
                width_texels: MAX_OUTLINE_TEXELS + 1.0,
            }),
            ..every_effect()
        },
        WorldEffects {
            bloom: Some(WorldBloom {
                strength: -1.0,
                radius_pixels: 4.0,
            }),
            ..every_effect()
        },
        WorldEffects {
            particles: Some(WorldParticles {
                count: MAX_PARTICLES + 1,
                radius_blocks: 1.0,
            }),
            ..every_effect()
        },
    ];
    for effects in bad {
        assert!(
            authority.validate_material(&with_effects(effects)).is_err(),
            "{effects:?}"
        );
    }
    for blend in [WorldBlend::Opaque, WorldBlend::Cutout] {
        let material = WorldMaterialSpec {
            blend,
            ..with_effects(every_effect())
        };
        assert!(
            authority.validate_material(&material).is_err(),
            "{blend:?} draws before the scene it would read"
        );
    }
}

#[test]
fn computes_need_passes_and_bound_their_cells_per_bundle() {
    let assets = BTreeSet::new();
    let compute = |cells| WorldComputeSpec {
        source: "fn step(input: WorldCellInput) -> vec4<f32> { return cell(input.index); }".into(),
        cells,
        params: vec![1.0],
    };
    let base = BTreeSet::from([Permission::RenderWorldMaterial]);
    assert!(
        authority(&base, &assets)
            .validate_compute(&compute(16))
            .is_err()
    );
    let passes = BTreeSet::from([
        Permission::RenderWorldMaterial,
        Permission::RenderWorldMaterialPasses,
    ]);
    let authority = authority(&passes, &assets);
    authority.validate_compute(&compute(16)).unwrap();
    for cells in [0, MAX_COMPUTE_CELLS + 1] {
        assert!(
            authority.validate_compute(&compute(cells)).is_err(),
            "{cells}"
        );
    }
    let mut world = WorldMaterials::default();
    let reader = with_effects(WorldEffects {
        cells: Some(3),
        ..WorldEffects::default()
    });
    assert!(
        world.set_material(1, Some(&reader)).is_err(),
        "a material reads only held cells"
    );
    world
        .set_compute(3, Some(&compute(MAX_COMPUTE_CELLS)))
        .unwrap();
    world.set_material(1, Some(&reader)).unwrap();
    assert!(world.set_compute(3, None).is_err(), "cells in use stay");
    // The bundle's computes together update a bounded number of cells a frame; replacing one
    // counts only its new size.
    let per_frame = MAX_BUNDLE_CELLS_PER_FRAME / MAX_COMPUTE_CELLS;
    for id in 4..3 + per_frame {
        world
            .set_compute(id, Some(&compute(MAX_COMPUTE_CELLS)))
            .unwrap();
    }
    assert!(world.set_compute(99, Some(&compute(1))).is_err());
    world
        .set_compute(3, Some(&compute(MAX_COMPUTE_CELLS)))
        .unwrap();
    assert_eq!(
        compute(MAX_COMPUTE_CELLS).cell_bytes(),
        u64::from(MAX_COMPUTE_CELLS) * 32
    );
    world.set_material(1, None).unwrap();
    world.set_compute(3, None).unwrap();
}

#[test]
fn withdrawing_a_permission_strips_exactly_its_effects() {
    let all = WorldEffects {
        cells: Some(2),
        ..every_effect()
    };
    assert_eq!(all.without(&BTreeSet::new()), all);
    let vertex = all.without(&BTreeSet::from([Permission::RenderWorldMaterialVertex]));
    assert_eq!(vertex.displacement_blocks, None);
    assert_eq!(
        WorldEffects {
            displacement_blocks: all.displacement_blocks,
            ..vertex
        },
        all
    );
    let scene = all.without(&BTreeSet::from([Permission::RenderWorldMaterialScene]));
    assert_eq!(
        WorldEffects {
            scene: true,
            ..scene
        },
        all
    );
    let passes = all.without(&BTreeSet::from([Permission::RenderWorldMaterialPasses]));
    assert_eq!(
        passes,
        WorldEffects {
            displacement_blocks: all.displacement_blocks,
            scene: true,
            ..WorldEffects::default()
        }
    );
    assert!(
        passes
            .permissions()
            .iter()
            .all(|p| *p != Permission::RenderWorldMaterialPasses)
    );
}
