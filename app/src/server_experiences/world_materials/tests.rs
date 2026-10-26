use super::*;
use mod_render::world_material::publication::Blend;
use server_experience::world_material::{WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec};
use std::io::Cursor;

const PLAIN: &str = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }";

/// A 10/20/30 opaque PNG of `width` by `height` texels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// A cutout material over `source` sampling `textures`, with two parameters.
fn spec(source: &str, textures: &[&str]) -> WorldMaterialSpec {
    WorldMaterialSpec {
        source: source.into(),
        textures: textures.iter().map(|path| (*path).to_owned()).collect(),
        params: vec![0.5, 2.0],
        depth: WorldDepth::Test,
        blend: WorldBlend::Cutout,
    }
}

/// A bundle's declarations: `materials` by id, then `bindings` as (id, target, material).
fn world(
    materials: &[(u32, WorldMaterialSpec)],
    bindings: &[(u32, WorldTarget, u32)],
) -> WorldMaterials {
    let mut world = WorldMaterials::default();
    for (id, spec) in materials {
        world.set_material(*id, Some(spec)).unwrap();
    }
    for (id, target, material) in bindings {
        world
            .set_binding(
                *id,
                Some(&WorldBinding {
                    target: target.clone(),
                    material: *material,
                }),
            )
            .unwrap();
    }
    world
}

/// A bundle's signed files holding `textures` by path.
fn files(textures: Vec<(String, Vec<u8>)>) -> Arc<Files> {
    Arc::new(Files {
        namespace: "example".into(),
        templates: Default::default(),
        textures,
    })
}

/// `world` as `example.bundle` declares it with a GPU share of `gpu_bytes` and an unbounded
/// memory share.
fn bundle<'a>(world: &'a WorldMaterials, files: &'a Arc<Files>, gpu_bytes: u64) -> BundleWorld<'a> {
    BundleWorld {
        bundle: "example.bundle",
        world,
        gpu_bytes,
        memory_bytes: u64::MAX,
        files,
    }
}

#[test]
fn declarations_publish_textures_params_and_bindings_and_keep_unchanged_keys() {
    let textures = files(vec![("textures/glow.png".to_owned(), png(4, 2))]);
    let held = world(
        &[(1, spec(PLAIN, &["textures/glow.png"]))],
        &[
            (0, WorldTarget::Entity("example:moth".into()), 1),
            (1, WorldTarget::Block("example:lamp".into()), 1),
        ],
    );
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(presenter.rebuild([bundle(&held, &textures, u64::MAX)]));
    let set = Arc::clone(presenter.set());
    let (slot, published) = set.entity("example:moth").unwrap();
    assert_eq!(set.block("example:lamp").map(|(slot, _)| slot), Some(slot));
    assert_eq!(
        (published.textures[0].width, published.textures[0].height),
        (4, 2)
    );
    assert_eq!(published.textures[0].rgba.len(), 4 * 2 * 4);
    assert_eq!(
        &presenter.params().values[&published.key][..3],
        &[0.5, 2.0, 0.0]
    );
    assert_eq!(published.blend, Blend::Cutout);
    // Rebuilding unchanged declarations publishes nothing and recompiles nothing.
    assert!(!presenter.rebuild([bundle(&held, &textures, u64::MAX)]));
    assert!(Arc::ptr_eq(&set, presenter.set()));
    // A replaced declaration gets a new key.
    let replaced = world(
        &[(
            1,
            spec(
                "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(1.0), 1.0, vec3<f32>(0.0)); }",
                &[],
            ),
        )],
        &[(0, WorldTarget::Entity("example:moth".into()), 1)],
    );
    assert!(presenter.rebuild([bundle(&replaced, &textures, u64::MAX)]));
    let key = presenter.set().entity("example:moth").unwrap().1.key;
    assert_ne!(key, published.key);
    assert!(presenter.set().block("example:lamp").is_none());
}

#[test]
fn failures_skip_the_material_with_one_diagnostic_and_ending_the_runtime_restores_bedrock() {
    let textures = files(vec![
        ("textures/glow.png".to_owned(), png(4, 4)),
        (
            "textures/huge.png".to_owned(),
            png(MAX_TEXTURE_EXTENT + 1, 1),
        ),
    ]);
    let storage = format!("@group(1) @binding(0) var<storage, read_write> v: array<f32>; {PLAIN}");
    let held = world(
        &[
            (1, spec(&storage, &[])),
            (2, spec(PLAIN, &["textures/huge.png"])),
            (3, spec(PLAIN, &["textures/glow.png"])),
        ],
        &[
            (0, WorldTarget::Entity("example:a".into()), 1),
            (1, WorldTarget::Entity("example:b".into()), 2),
            (2, WorldTarget::Entity("example:c".into()), 3),
        ],
    );
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([bundle(&held, &textures, u64::MAX)]);
    let set = presenter.set();
    assert!(
        set.entity("example:a").is_none(),
        "a hostile shader never draws"
    );
    assert!(
        set.entity("example:b").is_none(),
        "an oversized texture never uploads"
    );
    assert!(set.entity("example:c").is_some());
    assert_eq!(presenter.diagnostics().count(), 2);
    presenter.rebuild([bundle(&held, &textures, u64::MAX)]);
    assert_eq!(
        presenter.diagnostics().count(),
        2,
        "each failure is reported once"
    );
    // A GPU share too small for the texture and pipeline charge draws nothing.
    let mut tight = WorldMaterialPresenter::inline();
    tight.rebuild([bundle(&held, &textures, PIPELINE_CHARGE_BYTES)]);
    assert!(tight.set().published().next().is_none());
    assert!(tight.diagnostics().any(|line| line.contains("GPU limit")));
    // Revocation: no declarations publish an empty set.
    assert!(presenter.rebuild(std::iter::empty()));
    assert!(presenter.set().published().next().is_none() && presenter.set().entities.is_empty());
}

#[test]
fn materials_charge_the_signed_memory_limit_and_retired_pipelines_stay_charged() {
    let textures = files(vec![("textures/glow.png".to_owned(), png(8, 8))]);
    let held = world(&[(1, spec(PLAIN, &["textures/glow.png"]))], &[]);
    // The decoded texture alone outweighs a tiny memory share.
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([BundleWorld {
        memory_bytes: 64,
        ..bundle(&held, &textures, u64::MAX)
    }]);
    assert!(presenter.set().published().next().is_none());
    assert!(
        presenter
            .diagnostics()
            .any(|line| line.contains("memory limit"))
    );
    // Compile churn: every replacement keeps its pipelines charged, so a hostile guest that
    // keeps replacing a material runs out of GPU budget rather than compiling forever.
    let budget = (PIPELINE_CHARGE_BYTES + BIND_GROUP_CHARGE_BYTES) * 3;
    let mut churn = WorldMaterialPresenter::inline();
    let mut published = 0;
    for round in 0..6 {
        let source = format!(
            "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({round}.0), 1.0, vec3<f32>(0.0)); }}"
        );
        let held = world(&[(1, spec(&source, &[]))], &[]);
        churn.rebuild([bundle(&held, &textures, budget)]);
        published += churn.set().published().count();
    }
    assert_eq!(published, 3, "the fourth compile exceeds the GPU limit");
    assert!(churn.diagnostics().any(|line| line.contains("GPU limit")));
}

#[test]
fn a_native_compile_failure_is_reported_once_and_keeps_the_material_out_of_the_way() {
    let textures = files(Vec::new());
    let held = world(
        &[(1, spec(PLAIN, &[]))],
        &[(0, WorldTarget::Entity("example:moth".into()), 1)],
    );
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([bundle(&held, &textures, u64::MAX)]);
    let key = presenter.set().published().next().unwrap().key;
    for _ in 0..3 {
        presenter.note_compile_failures(|failed| failed == key);
    }
    let reports: Vec<_> = presenter.diagnostics().collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains("failed to compile"));
}

#[test]
fn new_parameters_keep_the_compiled_material_and_its_charge() {
    let textures = files(vec![("textures/glow.png".to_owned(), png(4, 2))]);
    let target = (0, WorldTarget::Entity("example:moth".into()), 1);
    let declare = |params: Vec<f32>| {
        world(
            &[(
                1,
                WorldMaterialSpec {
                    params,
                    ..spec(PLAIN, &["textures/glow.png"])
                },
            )],
            std::slice::from_ref(&target),
        )
    };
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(presenter.rebuild([bundle(&declare(vec![0.0]), &textures, u64::MAX)]));
    let first = presenter.set().entity("example:moth").unwrap().1.clone();
    // A damage flash every tick for far longer than the GPU share could hold new pipelines.
    let budget = PIPELINE_CHARGE_BYTES + BIND_GROUP_CHARGE_BYTES + 4 * 2 * 4;
    for step in 1..=200 {
        let held = declare(vec![step as f32]);
        assert!(presenter.rebuild([bundle(&held, &textures, budget)]));
        let (_, published) = presenter.set().entity("example:moth").unwrap();
        assert_eq!(published.key, first.key, "the shader is not recompiled");
        assert!(Arc::ptr_eq(&published.material, &first.material));
        assert_eq!(presenter.params().values[&published.key][0], step as f32);
    }
    assert_eq!(
        presenter.diagnostics().count(),
        0,
        "nothing exceeded the share"
    );
}

#[test]
fn no_count_caps_a_bundle_but_a_block_past_the_tag_width_is_reported() {
    let textures = files(Vec::new());
    let count = publication::MAX_BLOCK_SLOTS as u32 + 1;
    // Distinct sources, so each is its own material.
    let materials: Vec<(u32, WorldMaterialSpec)> = (0..count)
        .map(|id| {
            let source = format!(
                "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({id}.0 / 100.0), 1.0, vec3<f32>(0.0)); }}"
            );
            (id, spec(&source, &[]))
        })
        .collect();
    let bindings: Vec<(u32, WorldTarget, u32)> = (0..count)
        .map(|id| (id, WorldTarget::Block(format!("example:lamp_{id}")), id))
        .chain([(count, WorldTarget::Entity("example:moth".into()), count - 1)])
        .collect();
    let held = world(&materials, &bindings);
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(presenter.rebuild([bundle(&held, &textures, u64::MAX)]));
    let set = presenter.set();
    assert_eq!(
        set.published().count(),
        count as usize,
        "every material is published"
    );
    assert!(set.block("example:lamp_0").is_some());
    assert!(
        set.block(&format!("example:lamp_{}", count - 1)).is_none(),
        "a block cannot carry the last slot"
    );
    assert!(set.entity("example:moth").is_some(), "an entity can");
    assert!(
        presenter
            .diagnostics()
            .any(|line| line.contains("past the session's material slots"))
    );
}

#[test]
fn replacing_or_removing_a_material_the_budget_rejected_charges_no_retired_pipeline() {
    let textures = files(vec![("textures/glow.png".to_owned(), png(4, 2))]);
    let source = |round: u32| {
        format!(
            "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({round}.0), 1.0, vec3<f32>(0.0)); }}"
        )
    };
    let entity = |id: u32, material: u32| {
        (
            id,
            WorldTarget::Entity(format!("example:moth_{id}")),
            material,
        )
    };
    // The first material's texture leaves less than a pipeline charge for the others.
    let budget = (PIPELINE_CHARGE_BYTES + BIND_GROUP_CHARGE_BYTES) * 2 + 4 * 2 * 4 - 1;
    let mut presenter = WorldMaterialPresenter::inline();
    let first = (1, spec(&source(1), &["textures/glow.png"]));
    let held = world(
        &[first.clone(), (2, spec(&source(2), &[]))],
        &[entity(1, 1), entity(2, 2)],
    );
    presenter.rebuild([bundle(&held, &textures, budget)]);
    assert_eq!(presenter.set().published().count(), 1);
    // Replace the rejected second material and add a third the budget also rejects.
    let held = world(
        &[
            first,
            (2, spec(&source(3), &[])),
            (3, spec(&source(4), &[])),
        ],
        &[entity(1, 1), entity(2, 2), entity(3, 3)],
    );
    presenter.rebuild([bundle(&held, &textures, budget)]);
    assert_eq!(presenter.set().published().count(), 1);
    // Remove the first and third: only the first was ever drawn, so only it stays charged and
    // the replacement fits in what is left.
    let held = world(&[(2, spec(&source(3), &[]))], &[entity(2, 2)]);
    presenter.rebuild([bundle(&held, &textures, budget)]);
    presenter.rebuild([bundle(&held, &textures, budget)]);
    assert!(
        presenter.set().entity("example:moth_2").is_some(),
        "{:?}",
        presenter.diagnostics().collect::<Vec<_>>()
    );
}

/// A distinct plain material per `round`, so each is its own program.
fn distinct(round: u32) -> WorldMaterialSpec {
    spec(
        &format!(
            "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({round}.0), 1.0, vec3<f32>(0.0)); }}"
        ),
        &[],
    )
}

#[test]
fn textures_that_do_not_fit_the_shares_are_never_decoded_or_held() {
    // Eight materials, each with its own 64x64 texture, against a memory share for about two.
    let texels = 64 * 64 * 4;
    let textures = files(
        (0..8)
            .map(|index| (format!("textures/{index}.png"), png(64, 64)))
            .collect(),
    );
    let materials: Vec<(u32, WorldMaterialSpec)> = (0..8)
        .map(|index| {
            let path = format!("textures/{index}.png");
            (index, spec(PLAIN, &[path.as_str()]))
        })
        .collect();
    let held = world(&materials, &[]);
    let memory = 2 * texels + 4096;
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([BundleWorld {
        memory_bytes: memory,
        ..bundle(&held, &textures, u64::MAX)
    }]);
    assert_eq!(presenter.set().published().count(), 2);
    assert!(
        presenter.held_texels() <= memory,
        "the host holds {} decoded bytes against a share of {memory}",
        presenter.held_texels()
    );
    // The GPU share bounds decoding the same way.
    // One program: its pipelines once, then each material's texels and bind group.
    let gpu = PIPELINE_CHARGE_BYTES + 2 * (texels + BIND_GROUP_CHARGE_BYTES);
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([bundle(&held, &textures, gpu)]);
    assert_eq!(presenter.set().published().count(), 2);
    assert_eq!(presenter.held_texels(), 2 * texels);
    // A share that shrinks below a drawn material releases its decoded texels.
    presenter.rebuild([BundleWorld {
        memory_bytes: 64,
        ..bundle(&held, &textures, gpu)
    }]);
    assert!(presenter.set().published().next().is_none());
    assert_eq!(presenter.held_texels(), 0);
}

#[test]
fn ending_the_session_releases_retired_charges() {
    let textures = files(Vec::new());
    let budget = PIPELINE_CHARGE_BYTES * 2;
    let mut presenter = WorldMaterialPresenter::inline();
    for round in 0..3 {
        presenter.rebuild([bundle(
            &world(&[(1, distinct(round))], &[]),
            &textures,
            budget,
        )]);
    }
    assert!(
        presenter.set().published().next().is_none(),
        "the session's retired pipelines used up the share"
    );
    let revision = presenter.set().revision;
    assert!(presenter.end_session(), "ending publishes the empty set");
    assert!(presenter.set().revision > revision);
    // The bundle reconnects with the same share.
    assert!(presenter.rebuild([bundle(&world(&[(1, distinct(3))], &[]), &textures, budget)]));
    let key = presenter.set().published().next().unwrap().key;
    assert!(key > 3, "keys keep counting across sessions");
}

/// No signed files.
static NO_FILES: std::sync::LazyLock<Arc<Files>> = std::sync::LazyLock::new(|| files(Vec::new()));

/// `world` declared by the bundle `name` with unbounded shares and no textures.
fn named<'a>(name: &'a str, world: &'a WorldMaterials) -> BundleWorld<'a> {
    BundleWorld {
        bundle: name,
        ..bundle(world, &NO_FILES, u64::MAX)
    }
}

#[test]
fn slots_stay_with_their_bundle_so_stale_tags_never_draw_another_bundles_shader() {
    let lamp =
        |id, name: &str, material| (id, WorldTarget::Block(format!("example:{name}")), material);
    let first = world(&[(1, distinct(1))], &[lamp(0, "first", 1)]);
    let second = world(&[(1, distinct(2))], &[lamp(0, "second", 1)]);
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([named("a.bundle", &first), named("b.bundle", &second)]);
    let (a_slot, _) = presenter.set().block("example:first").unwrap();
    let (b_slot, b_material) = presenter.set().block("example:second").unwrap();
    let b_key = b_material.key;
    // The first bundle stops; its meshes keep their tag until they remesh.
    presenter.rebuild([named("b.bundle", &second)]);
    let set = presenter.set();
    assert_eq!(
        set.block("example:second").map(|(slot, _)| slot),
        Some(b_slot)
    );
    assert!(
        set.materials[usize::from(a_slot)].is_none(),
        "the stopped bundle's slot draws nothing"
    );
    // A new material of the second bundle takes a slot of its own, not the stopped one's.
    let more = world(
        &[(1, distinct(2)), (2, distinct(3))],
        &[lamp(0, "second", 1), lamp(1, "third", 2)],
    );
    presenter.rebuild([named("b.bundle", &more)]);
    let set = presenter.set();
    let (third, _) = set.block("example:third").unwrap();
    assert_ne!(third, a_slot);
    assert_eq!(set.block("example:second").unwrap().1.key, b_key);
    assert!(set.materials[usize::from(a_slot)].is_none());
}

/// The render world's observation number `count`, carrying the block slots in `mask`.
fn carried(count: u64, mask: u64) -> slots::Carried {
    slots::Carried { count, mask }
}

#[test]
fn a_freed_slot_goes_to_another_bundle_only_once_no_mesh_carries_its_tag() {
    let lamp =
        |id, name: &str, material| (id, WorldTarget::Block(format!("example:{name}")), material);
    let first = world(&[(1, distinct(1))], &[lamp(0, "first", 1)]);
    let second = world(&[(1, distinct(2))], &[lamp(0, "second", 1)]);
    let more = world(
        &[(1, distinct(2)), (2, distinct(3))],
        &[lamp(0, "second", 1), lamp(1, "third", 2)],
    );
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.carried = carried(9, 0);
    presenter.rebuild([named("a.bundle", &first)]);
    let (a_slot, _) = presenter.set().block("example:first").unwrap();
    // The first bundle stops at observation 10, while a mesh still carries its slot.
    presenter.carried = carried(10, 1 << a_slot);
    presenter.rebuild([named("b.bundle", &second)]);
    let (b_slot, _) = presenter.set().block("example:second").unwrap();
    assert_ne!(b_slot, a_slot);
    // The next observation shows no resident mesh carrying it, but an upload extracted before
    // the stop may not be resident yet, so the slot stays reserved.
    presenter.carried = carried(11, 0);
    presenter.rebuild([named("b.bundle", &more)]);
    let (third, _) = presenter.set().block("example:third").unwrap();
    assert_ne!(
        third, a_slot,
        "a queued stale upload keeps its slot reserved"
    );
    // Two observations after the stop, with no mesh carrying it, the slot is anyone's.
    presenter.carried = carried(12, 0);
    let fourth = world(
        &[(1, distinct(2)), (2, distinct(3)), (3, distinct(4))],
        &[
            lamp(0, "second", 1),
            lamp(1, "third", 2),
            lamp(2, "fourth", 3),
        ],
    );
    presenter.rebuild([named("b.bundle", &fourth)]);
    assert_eq!(
        presenter
            .set()
            .block("example:fourth")
            .map(|(slot, _)| slot),
        Some(a_slot)
    );
}

#[test]
fn entity_only_materials_leave_the_block_slots_to_block_bound_ones() {
    let count = publication::MAX_BLOCK_SLOTS as u32;
    // As many entity-only materials as there are block slots, then one bound to a block.
    let materials: Vec<(u32, WorldMaterialSpec)> =
        (0..=count).map(|id| (id, distinct(id))).collect();
    let bindings: Vec<(u32, WorldTarget, u32)> = (0..count)
        .map(|id| (id, WorldTarget::Entity(format!("example:moth_{id}")), id))
        .chain([(count, WorldTarget::Block("example:lamp".into()), count)])
        .collect();
    let held = world(&materials, &bindings);
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.rebuild([named("example.bundle", &held)]);
    let set = presenter.set();
    assert!(
        set.block("example:lamp").is_some(),
        "{:?}",
        presenter.diagnostics().collect::<Vec<_>>()
    );
    assert!((0..count).all(|id| set.entity(&format!("example:moth_{id}")).is_some()));
}

#[test]
fn a_new_session_keeps_off_the_slots_meshes_still_carry() {
    let lamp = |name: &str| (0, WorldTarget::Block(format!("example:{name}")), 1);
    let old = world(&[(1, distinct(1))], &[lamp("old")]);
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.carried = carried(4, 0);
    presenter.rebuild([named("example.bundle", &old)]);
    let (old_slot, _) = presenter.set().block("example:old").unwrap();
    presenter.carried = carried(5, 1 << old_slot);
    presenter.end_session();
    presenter.run = 2;
    // The same bundle in the next session binds another program.
    let new = world(&[(1, distinct(2))], &[lamp("new")]);
    presenter.rebuild([named("example.bundle", &new)]);
    assert_ne!(
        presenter.set().block("example:new").map(|(slot, _)| slot),
        Some(old_slot),
        "the old session's tag still on a mesh never draws the new program"
    );
}

#[test]
fn admission_runs_off_the_frame_and_publishes_once_done() {
    let held_jobs: Arc<std::sync::Mutex<Vec<admission::Job>>> = Arc::default();
    let run_held = || {
        let jobs = std::mem::take(&mut *held_jobs.lock().unwrap());
        let ran = jobs.len();
        for job in jobs {
            job();
        }
        ran
    };
    let textures = files(vec![("textures/glow.png".to_owned(), png(4, 2))]);
    let held = world(
        &[(1, spec(PLAIN, &["textures/glow.png"]))],
        &[(0, WorldTarget::Entity("example:moth".into()), 1)],
    );
    let mut presenter =
        WorldMaterialPresenter::with_executor(admission::Executor::Held(Arc::clone(&held_jobs)));
    presenter.rebuild([bundle(&held, &textures, u64::MAX)]);
    assert!(presenter.set().entity("example:moth").is_none());
    assert_eq!(presenter.diagnostics().count(), 0, "waiting is no failure");
    // The shader is validated and the texture's header read off the frame, then decoded.
    assert_eq!(run_held(), 1);
    presenter.rebuild([bundle(&held, &textures, u64::MAX)]);
    assert!(presenter.set().entity("example:moth").is_none());
    assert_eq!(presenter.held_texels(), 0);
    assert_eq!(run_held(), 1);
    assert_ne!(presenter.done.load(Ordering::Acquire), presenter.seen);
    assert!(presenter.rebuild([bundle(&held, &textures, u64::MAX)]));
    assert!(presenter.set().entity("example:moth").is_some());
    assert_eq!(run_held(), 0, "an unchanged rebuild starts no work");
}

/// A presenter whose admission jobs wait in `held` until the test runs them.
fn held(held: &Arc<std::sync::Mutex<Vec<admission::Job>>>) -> WorldMaterialPresenter {
    WorldMaterialPresenter::with_executor(admission::Executor::Held(Arc::clone(held)))
}

/// Runs the held jobs, the last `count` only when given; returns how many ran.
fn run_jobs(held: &Arc<std::sync::Mutex<Vec<admission::Job>>>, last: Option<usize>) -> usize {
    let mut jobs = held.lock().unwrap();
    let from = last.map_or(0, |count| jobs.len().saturating_sub(count));
    let ran: Vec<admission::Job> = jobs.drain(from..).collect();
    drop(jobs);
    let count = ran.len();
    for job in ran {
        job();
    }
    count
}

#[test]
fn one_admission_job_runs_per_bundle_and_churn_never_starves_an_older_material() {
    let jobs = Arc::default();
    let mut presenter = held(&jobs);
    let three = world(&[(1, distinct(1)), (2, distinct(2)), (3, distinct(3))], &[]);
    presenter.rebuild([named("example.bundle", &three)]);
    assert_eq!(presenter.jobs(), 1, "three new materials, one job");
    let oldest = presenter.cache[&("example.bundle".to_owned(), 1)].attempt;
    // A guest replacing material 3's program every callback starts no job of its own while
    // the oldest one runs.
    for round in 4..12 {
        let replaced = world(
            &[(1, distinct(1)), (2, distinct(2)), (3, distinct(round))],
            &[],
        );
        presenter.rebuild([named("example.bundle", &replaced)]);
        assert_eq!(presenter.jobs(), 1);
        assert_eq!(jobs.lock().unwrap().len(), 1, "nothing more was spawned");
        let running = presenter
            .cache
            .values()
            .find(|cached| cached.running())
            .unwrap();
        assert_eq!(running.attempt, oldest, "the oldest attempt runs");
    }
    // Once it returns, the next oldest measures, not the churning one.
    run_jobs(&jobs, None);
    let churned = world(
        &[(1, distinct(1)), (2, distinct(2)), (3, distinct(99))],
        &[],
    );
    presenter.rebuild([named("example.bundle", &churned)]);
    let running = presenter
        .cache
        .iter()
        .find(|(_, cached)| cached.running())
        .map(|((_, id), _)| *id);
    assert_eq!(running, Some(2));
}

#[test]
fn a_superseded_job_keeps_its_bundle_busy_until_it_returns_and_stops_early() {
    let jobs = Arc::default();
    let mut presenter = held(&jobs);
    let textures = files(vec![("textures/glow.png".to_owned(), png(4, 2))]);
    let first = world(&[(1, spec(PLAIN, &["textures/glow.png"]))], &[]);
    presenter.rebuild([bundle(&first, &textures, u64::MAX)]);
    run_jobs(&jobs, None);
    // Its decode is running when the guest replaces the material.
    presenter.rebuild([bundle(&first, &textures, u64::MAX)]);
    assert_eq!(presenter.jobs(), 1);
    let replaced = world(&[(1, distinct(5))], &[]);
    presenter.rebuild([bundle(&replaced, &textures, u64::MAX)]);
    assert_eq!(
        presenter.jobs(),
        0,
        "the replacement waits for the superseded job"
    );
    assert_eq!(jobs.lock().unwrap().len(), 1, "and nothing new was spawned");
    assert_eq!(presenter.orphans.len(), 1);
    // The superseded decode returns at once, seeing its cancel flag.
    run_jobs(&jobs, None);
    presenter.rebuild([bundle(&replaced, &textures, u64::MAX)]);
    assert!(presenter.orphans.is_empty());
    assert_eq!(presenter.jobs(), 1, "now the replacement measures");
}

#[test]
fn a_cancelled_decode_stops_before_its_next_texture() {
    let textures = files(vec![("textures/glow.png".to_owned(), png(4, 2))]);
    let paths = vec!["textures/glow.png".to_owned(); 2];
    let cancel = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(
        admission::decode_all(&paths, &textures, &cancel)
            .unwrap()
            .len(),
        2
    );
    cancel.store(true, Ordering::Release);
    assert!(admission::decode_all(&paths, &textures, &cancel).is_err());
}

#[test]
fn a_bundle_admits_nothing_new_until_every_material_is_measured() {
    let jobs = Arc::default();
    let mut presenter = held(&jobs);
    let two = world(&[(1, distinct(1)), (2, distinct(2))], &[]);
    presenter.rebuild([named("example.bundle", &two)]);
    // The newer material measures first; the other is not measured yet.
    assert_eq!(run_jobs(&jobs, Some(1)), 1);
    presenter.rebuild([named("example.bundle", &two)]);
    assert_eq!(
        jobs.lock().unwrap().len(),
        1,
        "only the other material's measure runs: nothing decodes before both are measured"
    );
    assert!(presenter.set().published().next().is_none());
    // Settled, both draw.
    for _ in 0..4 {
        run_jobs(&jobs, None);
        presenter.rebuild([named("example.bundle", &two)]);
    }
    assert_eq!(presenter.set().published().count(), 2);
}

#[test]
fn one_server_filling_its_pipeline_share_leaves_other_servers_theirs() {
    let room = (SERVER_PIPELINE_BYTES / PIPELINE_CHARGE_BYTES) as u32;
    let many: Vec<(u32, WorldMaterialSpec)> = (0..=room).map(|id| (id, distinct(id))).collect();
    let held = world(&many, &[]);
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.server = "first".into();
    presenter.rebuild([named("example.bundle", &held)]);
    // Unbound materials still draw; their parameters count them past the slot table.
    assert_eq!(presenter.params().values.len(), room as usize);
    assert!(
        presenter
            .diagnostics()
            .any(|line| line.contains("this server's share"))
    );
    // A later session of the same server still draws what it compiled, but no new program.
    presenter.end_session();
    presenter.run = 2;
    let later = world(&[(1, distinct(1)), (2, distinct(room + 1))], &[]);
    presenter.rebuild([named("example.bundle", &later)]);
    assert_eq!(presenter.params().values.len(), 1);
    // Another server compiles its own.
    presenter.end_session();
    presenter.run = 3;
    presenter.server = "second".into();
    presenter.rebuild([named("example.bundle", &later)]);
    assert_eq!(presenter.params().values.len(), 2);
}
