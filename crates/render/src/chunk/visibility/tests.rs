use std::{any::TypeId, collections::BTreeMap};

use bevy::{
    asset::AssetPlugin,
    camera::{
        Camera, CameraProjection, PerspectiveProjection,
        primitives::Frustum,
        visibility::{
            InheritedVisibility, NoCpuCulling, RenderLayers, ViewVisibility, Visibility,
            VisibilityClass, VisibilityPlugin, VisibleEntities, check_visibility_cpu_culling,
        },
    },
    ecs::system::RunSystemOnce,
    math::{Mat4, Vec3, Vec4},
    mesh::MeshPlugin,
    prelude::{App, Entity, GlobalTransform, MinimalPlugins, Transform, World},
    shape::{HalfSpace, ViewFrustum},
};
use render_scene::{ChunkRenderInstance, ChunkRenderQueue, ChunkUploadPriority};
use world::SubChunkKey;

use super::*;
use crate::chunk::{ChunkEntities, ChunkRenderPlugin};

/// Deterministic SplitMix64 stream, so a failing seed reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1) as u64) as i32
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }
}

#[derive(Clone, Copy, Debug)]
struct Section {
    origin: [i32; 3],
    models: bool,
    shown: bool,
}

/// Sections around `offset` (a multiple of 16), spread like a loaded render distance.
fn sections(rng: &mut Rng, offset: [i32; 3], count: usize, radius: i32) -> Vec<Section> {
    (0..count)
        .map(|_| Section {
            origin: [
                offset[0] + 16 * rng.range(-radius, radius),
                offset[1] + 16 * rng.range(-4, 19),
                offset[2] + 16 * rng.range(-radius, radius),
            ],
            models: rng.chance(1, 3),
            shown: rng.chance(5, 6),
        })
        .collect()
}

/// A world holding `sections` exactly as chunks were spawned for Bevy's CPU culling, and one
/// active camera.
fn bevy_reference(sections: &[Section]) -> (World, Entity, Vec<Entity>) {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let mut world = World::new();
    let entities = sections
        .iter()
        .map(|section| {
            let mut class = VisibilityClass::default();
            class.push(TypeId::of::<ChunkRenderInstance>());
            let [x, y, z] = section.origin.map(|value| value as f32);
            world
                .spawn((
                    class,
                    if section.shown {
                        InheritedVisibility::VISIBLE
                    } else {
                        InheritedVisibility::HIDDEN
                    },
                    ViewVisibility::default(),
                    GlobalTransform::from_xyz(x, y, z),
                    crate::chunk::bounds::aabb(section.models),
                ))
                .id()
        })
        .collect();
    let camera = world
        .spawn((
            Camera::default(),
            Frustum::default(),
            VisibleEntities::default(),
        ))
        .id();
    (world, camera, entities)
}

/// Bevy's own `check_visibility_cpu_culling` list for `frustum`.
fn bevy_list(world: &mut World, camera: Entity, frustum: Frustum) -> Vec<Entity> {
    *world.get_mut::<Frustum>(camera).unwrap() = frustum;
    world.run_system_once(check_visibility_cpu_culling).unwrap();
    world
        .get::<VisibleEntities>(camera)
        .unwrap()
        .get(TypeId::of::<ChunkRenderInstance>())
        .to_vec()
}

fn set_of(entities: &[Entity], sections: &[Section]) -> ChunkVisibilitySet {
    let mut set = ChunkVisibilitySet::default();
    for (&entity, section) in entities.iter().zip(sections) {
        set.insert(entity, section.origin, section.models, section.shown);
    }
    set
}

fn culled(set: &ChunkVisibilitySet, frustum: &Frustum) -> Vec<Entity> {
    let mut out = Vec::new();
    set.cull_into(frustum, &mut out, &mut CullScratch::default());
    out
}

/// Sections whose f32 plane test is too close to call, so Bevy's own test decides them.
fn exact_decisions(set: &ChunkVisibilitySet, frustum: &Frustum) -> usize {
    let planes = CullPlanes::new(frustum);
    set.blocks
        .iter()
        .map(|block| {
            let mut classes = vec![REJECT; block.entities.len()];
            planes.classify(block, &mut classes);
            classes.iter().filter(|&&class| class == UNSURE).count()
        })
        .sum()
}

fn transform(eye: Vec3, direction: Vec3) -> GlobalTransform {
    let up = if direction.cross(Vec3::Y).length_squared() < 1e-6 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    GlobalTransform::from(Transform::from_translation(eye).looking_to(direction, up))
}

/// A perspective camera frustum, built as Bevy's `update_frusta` builds it.
fn perspective(eye: Vec3, direction: Vec3, fov: f32, aspect_ratio: f32, near: f32) -> Frustum {
    PerspectiveProjection {
        fov,
        aspect_ratio,
        near,
        far: 4096.0,
        ..Default::default()
    }
    .compute_frustum(&transform(eye, direction))
}

/// The client's reverse-Z infinite projection.
fn reverse_infinite(eye: Vec3, direction: Vec3, fov: f32, aspect: f32) -> Frustum {
    let global = transform(eye, direction);
    let projection =
        glam::camera::rh::proj::directx::perspective_infinite_reverse(fov, aspect, 0.05);
    Frustum(ViewFrustum::from_clip_from_world_custom_far(
        &(projection * Mat4::from(global.affine().inverse())),
        &global.translation(),
        &global.back().as_vec3(),
        4096.0,
    ))
}

/// An orthographic box `low..high` in world space.
fn orthographic(low: Vec3, high: Vec3) -> Frustum {
    Frustum(ViewFrustum::from_clip_from_world(
        &glam::camera::rh::proj::directx::orthographic(
            low.x, high.x, low.y, high.y, -high.z, -low.z,
        ),
    ))
}

fn random_direction(rng: &mut Rng) -> Vec3 {
    loop {
        let direction = Vec3::new(
            rng.between(-1.0, 1.0),
            rng.between(-1.0, 1.0),
            rng.between(-1.0, 1.0),
        );
        if direction.length_squared() > 0.01 {
            return direction.normalize();
        }
    }
}

fn random_frustum(rng: &mut Rng, offset: [i32; 3], radius: i32) -> Frustum {
    let span = (16 * radius) as f32;
    let eye = Vec3::from_array(offset.map(|value| value as f32))
        + Vec3::new(
            rng.between(-span, span),
            rng.between(-64.0, 320.0),
            rng.between(-span, span),
        );
    let direction = random_direction(rng);
    match rng.below(4) {
        0 => perspective(
            eye,
            direction,
            rng.between(0.2, 2.6),
            rng.between(0.3, 3.5),
            rng.between(0.01, 2.0),
        ),
        1 => reverse_infinite(eye, direction, rng.between(0.2, 2.6), rng.between(0.3, 3.5)),
        2 => {
            let size = Vec3::new(
                rng.between(1.0, span),
                rng.between(1.0, 200.0),
                rng.between(1.0, span),
            );
            orthographic(eye - size, eye + size)
        }
        _ => {
            // A portal-style oblique near plane, as `PerspectiveProjection` supports.
            let normal =
                Vec3::new(rng.between(-0.5, 0.5), rng.between(-0.5, 0.5), -1.0).normalize();
            PerspectiveProjection {
                fov: rng.between(0.4, 2.0),
                aspect_ratio: rng.between(0.5, 2.5),
                near_clip_plane: normal.extend(-rng.between(0.05, 40.0)),
                ..Default::default()
            }
            .compute_frustum(&transform(eye, direction))
        }
    }
}

const OFFSETS: [[i32; 3]; 4] = [
    [0, 0, 0],
    [16 * 6_250, 0, -16 * 81_250],
    [-16 * 1_250_000, 0, 16 * 937_500],
    [16 * 1_874_999, 0, -16 * 1_874_999],
];

#[test]
fn lists_match_bevy_cpu_culling_for_random_cameras() {
    let mut rng = Rng(0x5EED_0001);
    for offset in OFFSETS {
        let scene = sections(&mut rng, offset, 2500, 12);
        let (mut world, camera, entities) = bevy_reference(&scene);
        let set = set_of(&entities, &scene);
        let (mut listed, mut exact) = (0, 0);
        for round in 0..120 {
            let frustum = random_frustum(&mut rng, offset, 12);
            let list = culled(&set, &frustum);
            assert_eq!(
                list,
                bevy_list(&mut world, camera, frustum),
                "offset {offset:?}, camera {round}"
            );
            listed += list.len();
            exact += exact_decisions(&set, &frustum);
        }
        assert!(listed > 0, "offset {offset:?} saw no section");
        if offset == [0; 3] {
            // Near the world origin the plane test decides all but a sliver of sections.
            assert!(exact * 1000 < scene.len() * 120, "{exact} exact decisions");
        }
    }
}

/// Planes through section faces, edges and corners put sections exactly on the boundary,
/// where only Bevy's own test decides.
#[test]
fn lists_match_bevy_cpu_culling_on_section_aligned_planes() {
    let mut rng = Rng(0x5EED_0002);
    for offset in OFFSETS {
        let base = Vec3::from_array(offset.map(|value| value as f32));
        let scene = sections(&mut rng, offset, 3000, 6);
        let (mut world, camera, entities) = bevy_reference(&scene);
        let set = set_of(&entities, &scene);
        let mut frusta = Vec::new();
        for _ in 0..40 {
            let low = Vec3::new(
                (16 * rng.range(-6, 4)) as f32,
                (16 * rng.range(-4, 10)) as f32,
                (16 * rng.range(-6, 4)) as f32,
            );
            let size = Vec3::new(
                (16 * rng.range(1, 6)) as f32,
                (16 * rng.range(1, 8)) as f32,
                (16 * rng.range(1, 6)) as f32,
            );
            // Face-aligned boxes, and boxes one model overhang inside the faces.
            frusta.push(orthographic(base + low, base + low + size));
            frusta.push(orthographic(
                base + low + Vec3::splat(2.0),
                base + low + size - Vec3::splat(2.0),
            ));
            let corner = base
                + Vec3::new(
                    (16 * rng.range(-3, 3)) as f32,
                    (16 * rng.range(0, 8)) as f32,
                    (16 * rng.range(-3, 3)) as f32,
                );
            let axes = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z, Vec3::NEG_Y];
            let direction = axes[rng.below(axes.len() as u64) as usize];
            // A square 90-degree view puts its side planes through section corners.
            frusta.push(perspective(
                corner,
                direction,
                std::f32::consts::FRAC_PI_2,
                1.0,
                0.1,
            ));
            frusta.push(reverse_infinite(
                corner,
                (direction + Vec3::Y).normalize(),
                std::f32::consts::FRAC_PI_2,
                1.0,
            ));
        }
        let mut exact = 0;
        for (round, frustum) in frusta.into_iter().enumerate() {
            assert_eq!(
                culled(&set, &frustum),
                bevy_list(&mut world, camera, frustum),
                "offset {offset:?}, frustum {round}"
            );
            exact += exact_decisions(&set, &frustum);
        }
        assert!(exact > 0, "offset {offset:?} left no section on a plane");
    }
}

/// Frusta whose planes Bevy cannot normalise test every section the way Bevy does.
#[test]
fn degenerate_frusta_match_bevy_cpu_culling() {
    let mut rng = Rng(0x5EED_0003);
    let scene = sections(&mut rng, [0; 3], 400, 4);
    let (mut world, camera, entities) = bevy_reference(&scene);
    let set = set_of(&entities, &scene);
    let mut degenerate = Frustum::default();
    degenerate.half_spaces[1] = HalfSpace::new(Vec4::new(0.0, 0.0, 0.0, 1.0));
    let mut nan = perspective(Vec3::ZERO, Vec3::X, 1.2, 1.5, 0.1);
    nan.half_spaces[2] = HalfSpace::new(Vec4::splat(f32::NAN));
    for frustum in [Frustum::default(), degenerate, nan] {
        assert_eq!(
            culled(&set, &frustum),
            bevy_list(&mut world, camera, frustum)
        );
    }
}

/// Every shown section, as Bevy lists them for a camera with `NoCpuCulling`.
#[test]
fn shown_list_is_every_shown_section_in_entity_order() {
    let mut rng = Rng(0x5EED_0004);
    let scene = sections(&mut rng, [0; 3], 700, 8);
    let (_, _, entities) = bevy_reference(&scene);
    let set = set_of(&entities, &scene);
    let mut expected: Vec<_> = entities
        .iter()
        .zip(&scene)
        .filter(|(_, section)| section.shown)
        .map(|(&entity, _)| entity)
        .collect();
    expected.sort_unstable();
    let mut out = Vec::new();
    set.shown_into(&mut out);
    assert_eq!(out, expected);
}

/// Entity ids with scattered generations and indices, so blocks split, insert in the middle
/// and compact.
fn random_entity(rng: &mut Rng) -> Entity {
    let index = rng.below(1 << 20) as u32;
    let generation = rng.below(4) as u32;
    Entity::from_bits(u64::from(index) | (u64::from(generation) << 32))
}

#[test]
fn incremental_updates_match_full_recomputation() {
    let mut rng = Rng(0x5EED_0005);
    let kinds = [
        crate::chunk::bounds::aabb(false),
        crate::chunk::bounds::aabb(true),
    ];
    let mut set = ChunkVisibilitySet::default();
    let mut reference = BTreeMap::<Entity, Section>::new();
    let mut removed = Vec::new();
    let frusta: Vec<_> = (0..4)
        .map(|_| random_frustum(&mut rng, [0; 3], 10))
        .collect();
    for step in 0..12_000 {
        let roll = rng.below(100);
        if roll < 45 || reference.is_empty() {
            let entity = random_entity(&mut rng);
            let section = sections(&mut rng, [0; 3], 1, 10)[0];
            set.insert(entity, section.origin, section.models, section.shown);
            reference.insert(entity, section);
        } else if roll < 70 {
            let at = rng.below(reference.len() as u64) as usize;
            let entity = *reference.keys().nth(at).unwrap();
            set.remove(entity);
            reference.remove(&entity);
            removed.push(entity);
        } else if roll < 80 {
            // Reinstated after removal, as when an instance is removed and inserted again.
            if let Some(entity) = removed.pop() {
                let section = sections(&mut rng, [0; 3], 1, 10)[0];
                set.insert(entity, section.origin, section.models, section.shown);
                reference.insert(entity, section);
            }
        } else if roll < 90 {
            let at = rng.below(reference.len() as u64) as usize;
            let (&entity, section) = reference.iter_mut().nth(at).unwrap();
            section.shown = rng.chance(1, 2);
            set.set_shown(entity, section.shown);
        } else {
            // A replaced instance at the same entity may move between bounds kinds.
            let at = rng.below(reference.len() as u64) as usize;
            let (&entity, section) = reference.iter_mut().nth(at).unwrap();
            section.models = !section.models;
            set.insert(entity, section.origin, section.models, section.shown);
        }
        // Removed and unknown entities ignore visibility changes.
        let stray = random_entity(&mut rng);
        if !reference.contains_key(&stray) {
            set.set_shown(stray, true);
        }
        if step % 97 == 0 {
            for frustum in &frusta {
                let expected: Vec<_> = reference
                    .iter()
                    .filter(|(_, section)| {
                        section.shown
                            && bevy_visible(
                                frustum,
                                Vec3::from_array(section.origin.map(|value| value as f32)),
                                &kinds[usize::from(section.models)],
                            )
                    })
                    .map(|(&entity, _)| entity)
                    .collect();
                assert_eq!(culled(&set, frustum), expected, "step {step}");
            }
            let mut shown = Vec::new();
            set.shown_into(&mut shown);
            assert_eq!(
                shown,
                reference
                    .iter()
                    .filter(|(_, section)| section.shown)
                    .map(|(&entity, _)| entity)
                    .collect::<Vec<_>>(),
                "step {step}"
            );
        }
    }
}

/// A queue-driven app with Bevy's visibility propagation and CPU culling installed.
fn culling_app() -> App {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), MeshPlugin))
        .add_plugins((VisibilityPlugin, ChunkRenderPlugin::new(8)));
    app
}

fn chunk_list(app: &App, camera: Entity) -> Vec<Entity> {
    app.world()
        .get::<VisibleEntities>(camera)
        .unwrap()
        .get(TypeId::of::<ChunkRenderInstance>())
        .to_vec()
}

fn sorted(mut entities: Vec<Entity>) -> Vec<Entity> {
    entities.sort_unstable();
    entities
}

#[test]
fn published_lists_follow_spawns_hidden_sections_and_despawns() {
    let mut app = culling_app();
    // Sections at x = 0, 16 and 32 reach into the box; the one at x = 48 does not.
    let frustum = orthographic(Vec3::new(-4.0, -4.0, -4.0), Vec3::new(36.0, 20.0, 20.0));
    let camera = app
        .world_mut()
        .spawn((Camera::default(), frustum, VisibleEntities::default()))
        .id();
    let layered = app
        .world_mut()
        .spawn((
            Camera::default(),
            frustum,
            VisibleEntities::default(),
            RenderLayers::layer(1),
        ))
        .id();
    let unculled = app
        .world_mut()
        .spawn((
            Camera::default(),
            frustum,
            VisibleEntities::default(),
            NoCpuCulling,
        ))
        .id();
    let keys = (0..4)
        .map(|x| SubChunkKey::new(0, x, 0, 0))
        .collect::<Vec<_>>();
    for &key in &keys {
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(
                key,
                crate::chunk::tests::solid_test_mesh(),
                ChunkUploadPriority::new(0.0),
            )
            .unwrap();
    }
    app.update();
    let entities = keys
        .iter()
        .map(|key| app.world().resource::<ChunkEntities>().0[key])
        .collect::<Vec<_>>();
    for &entity in &entities {
        let entity = app.world().entity(entity);
        assert!(entity.contains::<NoCpuCulling>());
        assert!(!entity.contains::<GlobalTransform>());
        assert!(!entity.contains::<bevy::camera::primitives::Aabb>());
    }
    assert_eq!(chunk_list(&app, camera), sorted(entities[..3].to_vec()));
    assert!(chunk_list(&app, layered).is_empty());
    assert_eq!(chunk_list(&app, unculled), sorted(entities.clone()));

    // Cave culling hides a section through `Visibility`.
    *app.world_mut().get_mut::<Visibility>(entities[1]).unwrap() = Visibility::Hidden;
    app.update();
    assert_eq!(
        chunk_list(&app, camera),
        sorted(vec![entities[0], entities[2]])
    );
    assert_eq!(
        chunk_list(&app, unculled),
        sorted(vec![entities[0], entities[2], entities[3]])
    );

    app.world_mut().despawn(entities[0]);
    *app.world_mut().get_mut::<Visibility>(entities[1]).unwrap() = Visibility::Inherited;
    app.update();
    assert_eq!(
        chunk_list(&app, camera),
        sorted(vec![entities[1], entities[2]])
    );

    // An inactive camera keeps its last list, as Bevy leaves it.
    app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove(keys[2])
        .unwrap();
    app.update();
    assert_eq!(
        chunk_list(&app, camera),
        sorted(vec![entities[1], entities[2]])
    );
    assert_eq!(
        chunk_list(&app, unculled),
        sorted(vec![entities[1], entities[3]])
    );
}
