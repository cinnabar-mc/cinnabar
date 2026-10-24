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
