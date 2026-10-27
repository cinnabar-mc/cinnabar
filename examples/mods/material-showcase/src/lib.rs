//! Retained drawing material showcase: two `create-material` shaders on retained drawings.
//! Four glowing waypoint beams stand around where the player first stood, each its own colour,
//! with pulses of light travelling up them; an animated, colour-cycling dashed outline marks the
//! block under the player's feet and follows them. Needs the `world_draw` and `render_material`
//! grants and, for the player's position, `players`. The host validates one material per
//! callback, so `init` creates the beam's and the first frame the outline's.

use mod_api::bindings::{
    Guest,
    cinnabar::extension::{gameplay, hud},
    cinnabar::world_draw::drawing::{
        self, Anchor, Blend, BlockAnchor, Cap, Depth, GeometrySpec, Handle, Instance, Join, Mesh,
        Polyline, Transform, Vertex, WidthMode, WorldAnchor,
    },
};
use std::cell::{Cell, RefCell};

/// The beam's shader, which `create-material` validates.
pub const BEAM: &str = include_str!("../shaders/beam.wgsl");
/// The outline's shader.
pub const OUTLINE: &str = include_str!("../shaders/outline.wgsl");
/// Beam height in blocks.
const BEAM_HEIGHT: f32 = 24.0;
/// Beam width in blocks.
const BEAM_WIDTH: f32 = 0.6;
/// Where the beams stand around the first position, in blocks, and their colours.
pub const WAYPOINTS: [([f64; 2], [f32; 4]); 4] = [
    ([6.0, 0.0], [1.0, 0.35, 0.1, 1.0]),
    ([0.0, 6.0], [0.2, 0.8, 1.0, 1.0]),
    ([-6.0, 0.0], [0.5, 1.0, 0.3, 1.0]),
    ([0.0, -6.0], [0.9, 0.3, 1.0, 1.0]),
];
/// Beam parameters: three pulses along it, one cycle per four seconds, emission 4.
const BEAM_PARAMS: [f32; 3] = [3.0, 1.0, 4.0];
/// Outline parameters: four dashes per block, two cycles per four seconds, emission 3.
const OUTLINE_PARAMS: [f32; 3] = [4.0, 2.0, 3.0];

/// What `init` created, used by every frame.
struct Created {
    beam: Handle,
    beam_geometry: Handle,
    outline_geometry: Handle,
}

thread_local! {
    static CREATED: RefCell<Option<Created>> = const { RefCell::new(None) };
    /// The outline's material, which the first gameplay frame creates.
    static OUTLINE_MATERIAL: Cell<Option<Handle>> = const { Cell::new(None) };
    /// Set once the first gameplay frame has placed the drawings, even when one failed, so a
    /// failure is shown once rather than every frame recreating the beams.
    static PLACED: Cell<bool> = const { Cell::new(false) };
    /// The outline instance, when placing it succeeded.
    static OUTLINE_INSTANCE: Cell<Option<Handle>> = const { Cell::new(None) };
}

/// One vertex of white geometry; the instance tint colours it.
fn vertex(position: (f32, f32, f32), uv: (f32, f32)) -> Vertex {
    Vertex {
        position,
        color: (1.0, 1.0, 1.0, 1.0),
        uv,
        data: (0.0, 0.0, 0.0, 0.0),
    }
}

/// Two crossed vertical quads, so the beam reads from every side: uv.x across, uv.y up.
fn beam_mesh() -> Mesh {
    let half = BEAM_WIDTH / 2.0;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (dx, dz) in [(half, 0.0), (0.0, half)] {
        let base = vertices.len() as u32;
        vertices.extend([
            vertex((-dx, 0.0, -dz), (0.0, 0.0)),
            vertex((dx, 0.0, dz), (1.0, 0.0)),
            vertex((dx, BEAM_HEIGHT, dz), (1.0, 1.0)),
            vertex((-dx, BEAM_HEIGHT, -dz), (0.0, 1.0)),
        ]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh { vertices, indices }
}

/// The top edge of a block, as a closed line just above its face.
fn outline_line() -> Polyline {
    let y = 1.02;
    Polyline {
        vertices: [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .into_iter()
            .map(|(x, z)| vertex((x, y, z), (0.0, 0.0)))
            .collect(),
        closed: true,
    }
}

const IDENTITY: Transform = Transform {
    translation: (0.0, 0.0, 0.0),
    rotation: (0.0, 0.0, 0.0, 1.0),
    scale: (1.0, 1.0, 1.0),
};

/// An instance of `geometry` drawn with `material` at `anchor`.
fn instance(
    geometry: Handle,
    material: Handle,
    params: &[f32],
    anchor: Anchor,
    tint: [f32; 4],
    width: f32,
) -> Instance {
    Instance {
        geometry,
        material: Some(material),
        clip: None,
        params: params.to_vec(),
        anchor,
        transform: IDENTITY,
        tint: (tint[0], tint[1], tint[2], tint[3]),
        depth: Depth::Test,
        blend: Blend::Additive,
        width,
        width_mode: WidthMode::World,
        join: Join::Miter,
        cap: Cap::Butt,
    }
}

/// The block under the feet of a player standing with their eye at `eye`.
fn feet_block(eye: &gameplay::Vector3) -> (i32, i32, i32) {
    (
        eye.x.floor() as i32,
        (eye.y - mod_api::view::STANDING_PLAYER_EYE_HEIGHT).floor() as i32 - 1,
        eye.z.floor() as i32,
    )
}

/// Shows `text` as the mod's label.
fn label(text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).take(120).collect();
    let _ = hud::set_label(&text);
}

struct MaterialShowcase;

impl Guest for MaterialShowcase {
    fn init() {
        let created = (|| {
            Ok::<_, drawing::DrawError>(Created {
                beam: drawing::create_material(BEAM)?,
                beam_geometry: drawing::create_geometry(&GeometrySpec::Triangles(beam_mesh()))?,
                outline_geometry: drawing::create_geometry(&GeometrySpec::Line(outline_line()))?,
            })
        })();
        match created {
            Ok(created) => {
                CREATED.with(|slot| *slot.borrow_mut() = Some(created));
                label("Material showcase: beams and outline ready");
            }
            Err(error) => label(&format!("Material showcase: {}", error.detail)),
        }
    }

    fn frame() {
        let Ok(Some(frame)) = gameplay::read_frame() else {
            return;
        };
        CREATED.with(|slot| {
            let slot = slot.borrow();
            let Some(created) = slot.as_ref() else {
                return;
            };
            let (x, y, z) = feet_block(&frame.eye);
            let Some(outline_material) =
                OUTLINE_MATERIAL
                    .get()
                    .or_else(|| match drawing::create_material(OUTLINE) {
                        Ok(material) => {
                            OUTLINE_MATERIAL.set(Some(material));
                            Some(material)
                        }
                        Err(error) => {
                            label(&format!("Material showcase: {}", error.detail));
                            None
                        }
                    })
            else {
                return;
            };
            let outline = instance(
                created.outline_geometry,
                outline_material,
                &OUTLINE_PARAMS,
                Anchor::Block(BlockAnchor {
                    dimension: frame.dimension,
                    position: (x, y, z),
                }),
                [1.0; 4],
                0.06,
            );
            if let Some(handle) = OUTLINE_INSTANCE.get() {
                let _ = drawing::update(handle, &outline);
            } else if !PLACED.replace(true) {
                // The first gameplay frame places the beams around the player.
                for ([dx, dz], colour) in WAYPOINTS {
                    let anchor = Anchor::World(WorldAnchor {
                        dimension: frame.dimension,
                        origin: (
                            f64::from(x) + 0.5 + dx,
                            f64::from(y) + 1.0,
                            f64::from(z) + 0.5 + dz,
                        ),
                    });
                    let beam = instance(
                        created.beam_geometry,
                        created.beam,
                        &BEAM_PARAMS,
                        anchor,
                        colour,
                        // Unused by triangles, but every instance has a positive width.
                        1.0,
                    );
                    if let Err(error) = drawing::create(&beam) {
                        label(&format!("Material showcase: {}", error.detail));
                    }
                }
                match drawing::create(&outline) {
                    Ok(handle) => OUTLINE_INSTANCE.set(Some(handle)),
                    Err(error) => label(&format!("Material showcase: {}", error.detail)),
                }
            }
        });
    }
}

mod_api::bindings::export!(MaterialShowcase with_types_in mod_api::bindings);
