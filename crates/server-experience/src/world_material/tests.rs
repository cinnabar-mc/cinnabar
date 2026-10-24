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
