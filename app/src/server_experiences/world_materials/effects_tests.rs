//! The presenter's effects: each published only while its permission is granted and not
//! withdrawn, withdrawn and restored in one publication each, and charged to the bundle.

use super::*;
use server_experience::world_material::{
    WorldBinding, WorldBlend, WorldBloom, WorldComputeSpec, WorldDepth, WorldOutline,
    WorldParticles,
};

/// A premultiplied glass material using every effect: it waves, reads the scene and its
/// cells, and draws an outline, a bloom and particles.
const GLASS: &str = "
fn shade(input: WorldMaterialInput) -> Surface {
    return Surface(scene_color(vec2<f32>(2.0, 0.0)) * cell(0u).rgb, 0.5, vec3<f32>(1.0));
}
fn displace(input: WorldVertexInput) -> vec3<f32> { return input.normal * parameter(0u); }
fn outline(input: WorldMaterialInput) -> vec3<f32> { return vec3<f32>(1.0); }
fn particle(input: WorldParticleInput) -> WorldParticle {
    return WorldParticle(cell(input.index).xyz, 1.0, vec3<f32>(1.0), 1.0);
}";

const STEP: &str = "fn step(input: WorldCellInput) -> vec4<f32> { return cell(input.index); }";

fn every_effect() -> WorldEffects {
    WorldEffects {
        displacement_blocks: Some(0.5),
        scene: true,
        outline: Some(WorldOutline { width_pixels: 1.0 }),
        bloom: Some(WorldBloom {
            strength: 1.0,
            radius_pixels: 4.0,
        }),
        particles: Some(WorldParticles {
            count: 64,
            radius_blocks: 1.0,
        }),
        cells: Some(7),
    }
}

/// The glass material's declaration over `source` with `effects`.
fn glass_spec(source: &str, effects: WorldEffects) -> WorldMaterialSpec {
    WorldMaterialSpec {
        source: source.into(),
        textures: Vec::new(),
        params: vec![0.25],
        depth: WorldDepth::Test,
        blend: WorldBlend::Premultiplied,
        effects,
    }
}

/// The glass material with every effect, bound to `example:glass`, and compute 7 it reads.
fn glass_world() -> WorldMaterials {
    let mut world = WorldMaterials::default();
    world
        .set_compute(
            7,
            Some(&WorldComputeSpec {
                source: STEP.into(),
                cells: 128,
                params: vec![1.0],
            }),
        )
        .unwrap();
    world
        .set_material(1, Some(&glass_spec(GLASS, every_effect())))
        .unwrap();
    world
        .set_binding(
            0,
            Some(&WorldBinding {
                target: WorldTarget::Block("example:glass".into()),
                material: 1,
            }),
        )
        .unwrap();
    world
}

/// No signed files.
static NO_FILES: std::sync::LazyLock<Arc<Files>> = std::sync::LazyLock::new(|| {
    Arc::new(Files {
        namespace: "example".into(),
        templates: Default::default(),
        textures: Vec::new(),
    })
});

fn every_permission() -> BTreeSet<Permission> {
    BTreeSet::from([
        Permission::RenderWorldMaterial,
        Permission::RenderWorldMaterialVertex,
        Permission::RenderWorldMaterialScene,
        Permission::RenderWorldMaterialPasses,
    ])
}

fn present(
    presenter: &mut WorldMaterialPresenter,
    world: &WorldMaterials,
    permissions: &BTreeSet<Permission>,
    withdrawn: &BTreeSet<Permission>,
) -> bool {
    presenter.rebuild([BundleWorld {
        bundle: "example.bundle",
        world,
        gpu_bytes: 128 << 20,
        memory_bytes: u64::MAX,
        files: &NO_FILES,
        permissions,
        withdrawn,
    }])
}

/// The published glass material's effects.
fn glass(presenter: &WorldMaterialPresenter) -> Effects {
    let diagnostics: Vec<_> = presenter.diagnostics().collect();
    presenter
        .set()
        .block("example:glass")
        .unwrap_or_else(|| panic!("{diagnostics:?}"))
        .1
        .effects
}

#[test]
fn every_effect_publishes_with_its_permissions_and_its_cells() {
    let world = glass_world();
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(present(
        &mut presenter,
        &world,
        &every_permission(),
        &BTreeSet::new()
    ));
    let effects = glass(&presenter);
    assert_eq!(effects.displacement_blocks, Some(0.5));
    assert!(effects.scene);
    assert_eq!(effects.outline_pixels, Some(1.0));
    assert_eq!(effects.cells, Some(0));
    let computes = &presenter.set().computes;
    assert_eq!(computes.len(), 1);
    assert_eq!((computes[0].cells, computes[0].params[0]), (128, 1.0));
    let material = &presenter.set().block("example:glass").unwrap().1.material;
    assert!(material.features.displace && material.features.cells);
    // A bundle never granted the passes permission publishes no compute, and the material
    // that reads its cells draws without them or its extra passes.
    let mut alone = WorldMaterialPresenter::inline();
    let no_passes = BTreeSet::from([
        Permission::RenderWorldMaterial,
        Permission::RenderWorldMaterialVertex,
        Permission::RenderWorldMaterialScene,
    ]);
    present(&mut alone, &world, &no_passes, &BTreeSet::new());
    assert!(alone.set().computes.is_empty());
    let effects = glass(&alone);
    assert_eq!(
        (effects.cells, effects.outline_pixels, effects.particles),
        (None, None, None)
    );
    assert!(effects.scene && effects.displacement_blocks.is_some());
}

#[test]
fn withdrawing_a_permission_drops_its_effects_at_once_and_restoring_brings_them_back() {
    // A material that only waves and reads the scene still compiles without either.
    let wave = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }
fn displace(input: WorldVertexInput) -> vec3<f32> { return input.normal * parameter(0u); }";
    let mut world = glass_world();
    world
        .set_material(
            1,
            Some(&glass_spec(
                wave,
                WorldEffects {
                    displacement_blocks: Some(1.0),
                    ..WorldEffects::default()
                },
            )),
        )
        .unwrap();
    let granted = every_permission();
    let mut presenter = WorldMaterialPresenter::inline();
    present(&mut presenter, &world, &granted, &BTreeSet::new());
    let before = presenter.set().block("example:glass").unwrap().1.key;
    assert_eq!(glass(&presenter).displacement_blocks, Some(1.0));
    let vertex = BTreeSet::from([Permission::RenderWorldMaterialVertex]);
    assert!(
        present(&mut presenter, &world, &granted, &vertex),
        "one publication drops it"
    );
    assert_eq!(glass(&presenter).displacement_blocks, None);
    assert!(
        presenter.set().computes.len() == 1,
        "withdrawing one permission leaves the others' work"
    );
    present(&mut presenter, &world, &granted, &BTreeSet::new());
    assert_eq!(glass(&presenter).displacement_blocks, Some(1.0));
    assert_ne!(
        presenter.set().block("example:glass").unwrap().1.key,
        before,
        "the restored program is compiled again"
    );
    // Withdrawing passes stops the bundle's computes too.
    let passes = BTreeSet::from([Permission::RenderWorldMaterialPasses]);
    present(&mut presenter, &world, &granted, &passes);
    assert!(presenter.set().computes.is_empty());
}

#[test]
fn effects_and_cells_are_charged_to_the_bundle() {
    let world = glass_world();
    let mut presenter = WorldMaterialPresenter::inline();
    let permissions = every_permission();
    let none = BTreeSet::new();
    // Just short of the material's charge: its pipelines for the shade, outline, bloom and
    // particle passes, its bind group, the shared scene and bloom charges, and the compute's
    // cells and pipeline.
    let compute = 128 * 32 + PIPELINE_CHARGE_BYTES;
    let material = 4 * PIPELINE_CHARGE_BYTES
        + BIND_GROUP_CHARGE_BYTES
        + SCENE_CHARGE_BYTES
        + EFFECTS_PASS_CHARGE_BYTES;
    let rebuild = |presenter: &mut WorldMaterialPresenter, gpu_bytes: u64| {
        presenter.rebuild([BundleWorld {
            bundle: "example.bundle",
            world: &world,
            gpu_bytes,
            memory_bytes: u64::MAX,
            files: &NO_FILES,
            permissions: &permissions,
            withdrawn: &none,
        }])
    };
    rebuild(&mut presenter, compute + material - 1);
    assert!(presenter.set().block("example:glass").is_none());
    assert_eq!(presenter.set().computes.len(), 1);
    let mut fresh = WorldMaterialPresenter::inline();
    rebuild(&mut fresh, compute + material);
    assert!(fresh.set().block("example:glass").is_some());
}

#[test]
fn a_compute_that_fails_to_compile_is_reported_once_however_many_frames_note_it() {
    let world = glass_world();
    let mut presenter = WorldMaterialPresenter::inline();
    present(
        &mut presenter,
        &world,
        &every_permission(),
        &BTreeSet::new(),
    );
    let key = presenter.set().computes[0].key;
    // Each frame notes the failure, and rebuilds in between keep the compute's attempt.
    for _ in 0..5 {
        presenter.note_compile_failures(|failed| failed == key);
        present(
            &mut presenter,
            &world,
            &every_permission(),
            &BTreeSet::from([Permission::RenderWorldMaterialVertex]),
        );
        present(
            &mut presenter,
            &world,
            &every_permission(),
            &BTreeSet::new(),
        );
    }
    let reports: Vec<_> = presenter
        .diagnostics()
        .filter(|line| line.contains("compute failed to compile"))
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
}

/// `world` declared by bundle `name` with every permission and room for every effect.
fn every_effect_bundle<'a>(
    name: &'a str,
    world: &'a WorldMaterials,
    permissions: &'a BTreeSet<Permission>,
) -> BundleWorld<'a> {
    static NONE: BTreeSet<Permission> = BTreeSet::new();
    BundleWorld {
        bundle: name,
        world,
        gpu_bytes: 128 << 20,
        memory_bytes: u64::MAX,
        files: &NO_FILES,
        permissions,
        withdrawn: &NONE,
    }
}

/// The key of the compute whose cells the published glass material reads.
fn glass_cells(presenter: &WorldMaterialPresenter) -> Option<u64> {
    let index = glass(presenter).cells?;
    Some(presenter.set().computes.get(usize::from(index))?.key)
}

/// A compute declaration that only keeps its cells.
fn keeping(cells: u32) -> WorldComputeSpec {
    WorldComputeSpec {
        source: STEP.into(),
        cells,
        params: Vec::new(),
    }
}

#[test]
fn a_kept_material_follows_its_compute_when_an_earlier_one_is_removed() {
    let permissions = every_permission();
    let mut world = glass_world();
    world.set_compute(3, Some(&keeping(64))).unwrap();
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([every_effect_bundle("example.bundle", &world, &permissions)]);
    let read = glass_cells(&presenter).expect("the glass reads its cells");
    let key = presenter.set().block("example:glass").unwrap().1.key;
    world.set_compute(3, None).unwrap();
    presenter.rebuild([every_effect_bundle("example.bundle", &world, &permissions)]);
    assert_eq!(
        presenter.set().block("example:glass").unwrap().1.key,
        key,
        "the material itself is kept"
    );
    assert_eq!(
        glass_cells(&presenter),
        Some(read),
        "it still reads compute 7's cells"
    );
}

#[test]
fn a_material_never_reads_another_bundles_cells_when_an_earlier_bundle_stops() {
    let permissions = every_permission();
    let only_cells = |id| {
        let mut world = WorldMaterials::default();
        world.set_compute(id, Some(&keeping(32))).unwrap();
        world
    };
    let (first, drawn, last) = (only_cells(1), glass_world(), only_cells(2));
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([
        every_effect_bundle("a.bundle", &first, &permissions),
        every_effect_bundle("b.bundle", &drawn, &permissions),
        every_effect_bundle("c.bundle", &last, &permissions),
    ]);
    let own = glass_cells(&presenter).expect("the glass reads its own bundle's cells");
    // The first bundle stops, so every later compute moves down one place.
    presenter.rebuild([
        every_effect_bundle("b.bundle", &drawn, &permissions),
        every_effect_bundle("c.bundle", &last, &permissions),
    ]);
    let last_key = presenter.set().computes.last().unwrap().key;
    assert_ne!(
        glass_cells(&presenter),
        Some(last_key),
        "never the next bundle's cells"
    );
    assert_eq!(glass_cells(&presenter), Some(own));
}

#[test]
fn toggling_a_permission_near_the_gpu_limit_reuses_both_forms_and_keeps_the_computes() {
    let wave = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }
fn displace(input: WorldVertexInput) -> vec3<f32> { return input.normal * parameter(0u); }";
    let mut world = glass_world();
    world
        .set_material(
            1,
            Some(&glass_spec(
                wave,
                WorldEffects {
                    displacement_blocks: Some(1.0),
                    ..WorldEffects::default()
                },
            )),
        )
        .unwrap();
    let permissions = every_permission();
    // Room for the compute and the material in both forms, its displacing one and the one
    // without, and no more: a third set of pipelines would not fit.
    let gpu_bytes = 128 * 32 + 3 * PIPELINE_CHARGE_BYTES + BIND_GROUP_CHARGE_BYTES;
    let rebuild = |presenter: &mut WorldMaterialPresenter, withdrawn: &BTreeSet<Permission>| {
        presenter.rebuild([BundleWorld {
            bundle: "example.bundle",
            world: &world,
            gpu_bytes,
            memory_bytes: u64::MAX,
            files: &NO_FILES,
            permissions: &permissions,
            withdrawn,
        }])
    };
    let mut presenter = WorldMaterialPresenter::inline();
    rebuild(&mut presenter, &BTreeSet::new());
    let compute = presenter.set().computes[0].key;
    let vertex = BTreeSet::from([Permission::RenderWorldMaterialVertex]);
    for _ in 0..3 {
        rebuild(&mut presenter, &vertex);
        assert_eq!(glass(&presenter).displacement_blocks, None);
        rebuild(&mut presenter, &BTreeSet::new());
        assert_eq!(
            glass(&presenter).displacement_blocks,
            Some(1.0),
            "the restored form reuses its pipelines at no new charge"
        );
    }
    let passes = BTreeSet::from([Permission::RenderWorldMaterialPasses]);
    rebuild(&mut presenter, &passes);
    assert!(presenter.set().computes.is_empty());
    rebuild(&mut presenter, &BTreeSet::new());
    assert_eq!(
        presenter.set().computes.first().map(|compute| compute.key),
        Some(compute),
        "a withdrawn compute stays cached and comes back as it was"
    );
}
