//! The world material showcase's guest shaders, rastered through the host's own guest library
//! and output bounding, each checked for the pixels that tell its effect apart. `flat` rasters a
//! surface spanning the frame, as mobs and faces see it; `scene` traces a camera's rays to a
//! block cluster, so volumetric effects are checked from real viewpoints.

mod blocks;
mod effects;
mod flat;
mod mobs;
mod scene;
mod volumes;
mod wing;

/// The showcase's shader directory, in the Experience's client part.
pub(super) const SHADERS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/experiences/material-fx/client/shaders"
);

/// One showcase shader as the client part declares it: the shared prelude, then its own
/// source.
macro_rules! shader {
    ($name:literal) => {
        concat!(
            include_str!(
                "../../../../../examples/experiences/material-fx/client/shaders/common.wgsl"
            ),
            include_str!(concat!(
                "../../../../../examples/experiences/material-fx/client/shaders/",
                $name,
                ".wgsl"
            ))
        )
    };
}
pub(super) use shader;

/// The texture slots `source` reads: one more than the highest `texture(Nu, ...)` index.
fn declared_textures(source: &str) -> usize {
    source
        .match_indices("texture(")
        .filter_map(|(at, _)| {
            let rest = &source[at + "texture(".len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            (!digits.is_empty() && rest[digits.len()..].starts_with('u'))
                .then(|| digits.parse::<usize>().unwrap() + 1)
        })
        .max()
        .unwrap_or(0)
}

/// The effects a showcase material's source uses: the functions it defines and the
/// resources it reads.
fn used_features(source: &str) -> mod_render::world_material::Features {
    mod_render::world_material::Features {
        displace: source.contains("fn displace("),
        scene: source.contains("scene_color(") && !source.contains("fn scene_color(")
            || source.contains("scene_depth(") && !source.contains("fn scene_depth("),
        cells: source.contains(" cell("),
        outline: source.contains("fn outline("),
        particles: source.contains("fn particle("),
    }
}

/// Every showcase shader passes the host's sandbox with its prelude: no loops, at most 32
/// texture reads and 4096 expressions per pixel in the worst case, which bounds the work of
/// every copy of every effect, the raymarched ones included. Each material passes both with
/// the effects it uses and without any, as its plain form and its withdrawn form compile;
/// each compute step (`*_step`, without the prelude) passes the compute sandbox.
#[test]
fn every_showcase_shader_passes_the_sandbox() {
    let prelude = std::fs::read_to_string(format!("{SHADERS}/common.wgsl")).unwrap();
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(SHADERS).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let own = std::fs::read_to_string(&path).unwrap();
        if name == "common" {
            continue;
        }
        if name.ends_with("_step") {
            if let Err(error) = mod_render::world_material::compute::compose(&own) {
                failures.push(format!("{name}: {error}"));
            }
            continue;
        }
        let source = format!("{prelude}{own}");
        let textures = declared_textures(&source);
        for features in [Default::default(), used_features(&source)] {
            if let Err(error) =
                mod_render::world_material::compose_with(&source, textures, features)
            {
                failures.push(format!("{name} {features:?}: {error}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
