//! The world material showcase's guest shaders, rastered through the host's own guest library
//! and output bounding, each checked for the pixels that tell its effect apart. `flat` rasters a
//! surface spanning the frame, as mobs and faces see it; `scene` traces a camera's rays to a
//! block cluster, so volumetric effects are checked from real viewpoints.

mod blocks;
mod flat;
mod mobs;
mod scene;
mod volumes;

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

/// Every showcase shader passes the host's sandbox with its prelude: no loops, at most 32
/// texture reads and 4096 expressions per pixel in the worst case, which bounds the work of
/// every copy of every effect, the raymarched ones included.
#[test]
fn every_showcase_shader_passes_the_sandbox() {
    let prelude = std::fs::read_to_string(format!("{SHADERS}/common.wgsl")).unwrap();
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(SHADERS).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        if name == "common" {
            continue;
        }
        let source = format!("{prelude}{}", std::fs::read_to_string(&path).unwrap());
        if let Err(error) = mod_render::world_material::compose(&source, declared_textures(&source))
        {
            failures.push(format!("{name}: {error}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
