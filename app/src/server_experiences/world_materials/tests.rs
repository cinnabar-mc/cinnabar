use super::*;
use server_experience::world_material::WorldBinding;

const PLAIN: &str = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }";

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn spec(source: &str, textures: &[&str]) -> WorldMaterialSpec {
    WorldMaterialSpec {
        source: source.into(),
        textures: textures.iter().map(|path| (*path).to_owned()).collect(),
        params: vec![0.5, 2.0],
        depth: WorldDepth::Test,
        blend: WorldBlend::Cutout,
    }
}

fn world(
    materials: &[(u32, WorldMaterialSpec)],
    bindings: &[(u32, WorldTarget, u32)],
) -> WorldMaterials {
    WorldMaterials {
        materials: materials.iter().cloned().collect(),
        bindings: bindings
            .iter()
            .map(|(id, target, material)| {
                (
                    *id,
                    WorldBinding {
                        target: target.clone(),
                        material: *material,
                    },
                )
            })
            .collect(),
    }
}

fn bundle<'a>(
    world: &'a WorldMaterials,
    textures: &'a [(String, Vec<u8>)],
    gpu_bytes: u64,
) -> BundleWorld<'a> {
    BundleWorld {
        bundle: "example.bundle",
        world,
        gpu_bytes,
        memory_bytes: u64::MAX,
        textures,
    }
}

#[test]
fn declarations_publish_textures_params_and_bindings_and_keep_unchanged_keys() {
    let textures = vec![("textures/glow.png".to_owned(), png(4, 2))];
    let held = world(
        &[(1, spec(PLAIN, &["textures/glow.png"]))],
        &[
            (0, WorldTarget::Entity("example:moth".into()), 1),
            (1, WorldTarget::Block("example:lamp".into()), 1),
        ],
    );
    let mut presenter = WorldMaterialPresenter::default();
    assert!(presenter.rebuild([bundle(&held, &textures, u64::MAX)]));
    let set = Arc::clone(presenter.set());
    let (slot, published) = set.entity("example:moth").unwrap();
    assert_eq!(set.block("example:lamp").map(|(slot, _)| slot), Some(slot));
    assert_eq!(
        (published.textures[0].width, published.textures[0].height),
        (4, 2)
    );
    assert_eq!(published.textures[0].rgba.len(), 4 * 2 * 4);
    assert_eq!(&published.params[..3], &[0.5, 2.0, 0.0]);
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
    let textures = vec![
        ("textures/glow.png".to_owned(), png(4, 4)),
        (
            "textures/huge.png".to_owned(),
            png(MAX_TEXTURE_EXTENT + 1, 1),
        ),
    ];
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
    let mut presenter = WorldMaterialPresenter::default();
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
    let mut tight = WorldMaterialPresenter::default();
    tight.rebuild([bundle(&held, &textures, PIPELINE_CHARGE_BYTES)]);
    assert!(tight.set().materials.is_empty());
    assert!(tight.diagnostics().any(|line| line.contains("GPU limit")));
    // Revocation: no declarations publish an empty set.
    assert!(presenter.rebuild(std::iter::empty()));
    assert!(presenter.set().materials.is_empty() && presenter.set().entities.is_empty());
}

#[test]
fn materials_charge_the_signed_memory_limit_and_retired_pipelines_stay_charged() {
    let textures = vec![("textures/glow.png".to_owned(), png(8, 8))];
    let held = world(&[(1, spec(PLAIN, &["textures/glow.png"]))], &[]);
    // The decoded texture alone outweighs a tiny memory share.
    let mut presenter = WorldMaterialPresenter::default();
    presenter.rebuild([BundleWorld {
        memory_bytes: 64,
        ..bundle(&held, &textures, u64::MAX)
    }]);
    assert!(presenter.set().materials.is_empty());
    assert!(
        presenter
            .diagnostics()
            .any(|line| line.contains("memory limit"))
    );
    // Compile churn: every replacement keeps its pipelines charged, so a hostile guest that
    // keeps replacing a material runs out of GPU budget rather than compiling forever.
    let budget = PIPELINE_CHARGE_BYTES * 3;
    let mut churn = WorldMaterialPresenter::default();
    let mut published = 0;
    for round in 0..6 {
        let source = format!(
            "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({round}.0), 1.0, vec3<f32>(0.0)); }}"
        );
        let held = world(&[(1, spec(&source, &[]))], &[]);
        churn.rebuild([bundle(&held, &textures, budget)]);
        published += churn.set().materials.len();
    }
    assert_eq!(published, 3, "the fourth compile exceeds the GPU limit");
    assert!(churn.diagnostics().any(|line| line.contains("GPU limit")));
}

#[test]
fn a_native_compile_failure_is_reported_once_and_keeps_the_material_out_of_the_way() {
    let textures = Vec::new();
    let held = world(
        &[(1, spec(PLAIN, &[]))],
        &[(0, WorldTarget::Entity("example:moth".into()), 1)],
    );
    let mut presenter = WorldMaterialPresenter::default();
    presenter.rebuild([bundle(&held, &textures, u64::MAX)]);
    let key = presenter.set().materials[0].key;
    for _ in 0..3 {
        presenter.note_compile_failures(|failed| failed == key);
    }
    let reports: Vec<_> = presenter.diagnostics().collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains("failed to compile"));
}

#[test]
fn new_parameters_keep_the_compiled_material_and_its_charge() {
    let textures = vec![("textures/glow.png".to_owned(), png(4, 2))];
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
    let mut presenter = WorldMaterialPresenter::default();
    assert!(presenter.rebuild([bundle(&declare(vec![0.0]), &textures, u64::MAX)]));
    let first = presenter.set().entity("example:moth").unwrap().1.clone();
    // A damage flash every tick for far longer than the GPU share could hold new pipelines.
    let budget = PIPELINE_CHARGE_BYTES + 4 * 2 * 4;
    for step in 1..=200 {
        let held = declare(vec![step as f32]);
        assert!(presenter.rebuild([bundle(&held, &textures, budget)]));
        let (_, published) = presenter.set().entity("example:moth").unwrap();
        assert_eq!(published.key, first.key, "the shader is not recompiled");
        assert!(Arc::ptr_eq(&published.material, &first.material));
        assert_eq!(published.params[0], step as f32);
    }
    assert_eq!(
        presenter.diagnostics().count(),
        0,
        "nothing exceeded the share"
    );
}

#[test]
fn no_count_caps_a_bundle_but_a_block_past_the_tag_width_is_reported() {
    let textures = Vec::new();
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
    let mut presenter = WorldMaterialPresenter::default();
    assert!(presenter.rebuild([bundle(&held, &textures, u64::MAX)]));
    let set = presenter.set();
    assert_eq!(
        set.materials.len(),
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
    let textures = vec![("textures/glow.png".to_owned(), png(4, 2))];
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
    let budget = PIPELINE_CHARGE_BYTES * 2;
    let mut presenter = WorldMaterialPresenter::default();
    let first = (1, spec(&source(1), &["textures/glow.png"]));
    let held = world(
        &[first.clone(), (2, spec(&source(2), &[]))],
        &[entity(1, 1), entity(2, 2)],
    );
    presenter.rebuild([bundle(&held, &textures, budget)]);
    assert_eq!(presenter.set().materials.len(), 1);
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
    assert_eq!(presenter.set().materials.len(), 1);
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

#[test]
fn textures_that_do_not_fit_the_shares_are_never_decoded_or_held() {
    // Eight materials, each with its own 64x64 texture, against a memory share for about two.
    let texels = 64 * 64 * 4;
    let textures: Vec<(String, Vec<u8>)> = (0..8)
        .map(|index| (format!("textures/{index}.png"), png(64, 64)))
        .collect();
    let materials: Vec<(u32, WorldMaterialSpec)> = (0..8)
        .map(|index| {
            let path = format!("textures/{index}.png");
            (index, spec(PLAIN, &[path.as_str()]))
        })
        .collect();
    let held = world(&materials, &[]);
    let memory = 2 * texels + 4096;
    let mut presenter = WorldMaterialPresenter::default();
    presenter.rebuild([BundleWorld {
        memory_bytes: memory,
        ..bundle(&held, &textures, u64::MAX)
    }]);
    assert_eq!(presenter.set().materials.len(), 2);
    assert!(
        presenter.held_texels() <= memory,
        "the host holds {} decoded bytes against a share of {memory}",
        presenter.held_texels()
    );
    // The GPU share bounds decoding the same way.
    let gpu = 2 * (texels + PIPELINE_CHARGE_BYTES);
    let mut presenter = WorldMaterialPresenter::default();
    presenter.rebuild([bundle(&held, &textures, gpu)]);
    assert_eq!(presenter.set().materials.len(), 2);
    assert_eq!(presenter.held_texels(), 2 * texels);
    // A share that shrinks below a drawn material releases its decoded texels.
    presenter.rebuild([BundleWorld {
        memory_bytes: 64,
        ..bundle(&held, &textures, gpu)
    }]);
    assert!(presenter.set().materials.is_empty());
    assert_eq!(presenter.held_texels(), 0);
}
