//! A retained, hard-coded three-by-three chunk view through a world-depth-tested mesh clip.
use mod_api::{
    PlayerMod,
    bindings::cinnabar::extension::{gameplay, hud, input},
    drawing::{self, api::*},
};
use std::cell::RefCell;

const SIZE: (u32, u32, u32) = (48, 64, 48);
struct GeometrySet {
    grid: Handle,
    frame: Handle,
    plane: Handle,
}
struct Inputs {
    plane: GeometrySpec,
    frame: GeometrySpec,
    grid: GeometrySpec,
}
struct Scene {
    session: u64,
    dimension: i32,
    grid: Handle,
    placement: Instance,
    draw: Option<Handle>,
    handles: Vec<Handle>,
}
thread_local! {
    static SCENE:RefCell<Option<Scene>>=const {RefCell::new(None)};
    static GEOMETRY:RefCell<Option<GeometrySet>>=const {RefCell::new(None)};
    static INPUTS:RefCell<Option<Inputs>>=const {RefCell::new(None)};
}
struct Portal;

/// Gives each named block an exact canonical state description in the active registry.
fn named(name: &str, states: &str) -> BlockState {
    BlockState::Named(NamedBlock {
        name: name.into(),
        states: states.into(),
    })
}

/// Supplies air, a rough netherrack floor, columns and two animated lava pools.
fn grid() -> BlockGrid {
    let mut cells = Vec::with_capacity((SIZE.0 * SIZE.1 * SIZE.2) as usize);
    for x in 0..SIZE.0 {
        for z in 0..SIZE.2 {
            for y in 0..SIZE.1 {
                let ground = 3 + (x / 7 + z / 9) % 3;
                let pillar = (x % 13 == 3 || x % 13 == 4) && (z % 17 == 5 || z % 17 == 6) && y < 18;
                let pool = (8..36).contains(&x) && (10..42).contains(&z);
                let palette = if y < ground || pillar {
                    1
                } else if pool && y < 6 {
                    2
                } else {
                    0
                };
                cells.push(
                    drawing::pack_block_cell(BlockCell {
                        palette,
                        sky_light: Some(11),
                        block_light: Some(if palette == 2 { 15 } else { 7 }),
                    })
                    .expect("bounded example cell"),
                );
            }
        }
    }
    BlockGrid {
        size: SIZE,
        palette: vec![
            named("minecraft:air", "{}"),
            named("minecraft:netherrack", "{}"),
            named(
                "minecraft:lava",
                r#"{"liquid_depth":{"type":"int","value":0}}"#,
            ),
        ],
        cells,
    }
}

/// Creates a six-block-wide frame whose open middle is the clip's four-by-five rectangle.
fn frame_grid() -> BlockGrid {
    let mut cells = Vec::new();
    for x in 0..6 {
        for y in 0..7 {
            cells.push(
                drawing::pack_block_cell(BlockCell {
                    palette: u32::from(x == 0 || x == 5 || y == 0 || y == 6),
                    sky_light: None,
                    block_light: None,
                })
                .expect("bounded example frame"),
            );
        }
    }
    BlockGrid {
        size: (6, 7, 1),
        palette: vec![
            named("minecraft:air", "{}"),
            named("minecraft:obsidian", "{}"),
        ],
        cells,
    }
}

/// Positions a fixed portal in front of the player when this session starts.
fn create_scene(frame: &gameplay::Snapshot, geometry: &GeometrySet) -> Result<Scene, DrawError> {
    let origin = (
        f64::from(frame.eye.x - frame.yaw.sin() * 8.),
        f64::from(frame.eye.y - 2.5),
        f64::from(frame.eye.z - frame.yaw.cos() * 8.),
    );
    let anchor = Anchor::World(WorldAnchor {
        dimension: frame.dimension,
        origin,
    });
    let rotation = (0., (frame.yaw / 2.).sin(), 0., (frame.yaw / 2.).cos());
    let transform = Transform {
        translation: (0., 0., 0.),
        rotation,
        scale: (1., 1., 1.),
    };
    let mut handles = Vec::new();
    let result = (|| {
        let plane = geometry.plane;
        let clip = create_clip(ClipSpec {
            geometry: plane,
            anchor,
            transform,
            parent: None,
        })?;
        handles.push(clip);
        let frame_geometry = geometry.frame;
        let mut frame_draw = drawing::instance(frame_geometry, anchor);
        frame_draw.transform = transform;
        let (s, c) = frame.yaw.sin_cos();
        frame_draw.transform.translation = (-3. * c - 0.5 * s, -1., 3. * s - 0.5 * c);
        handles.push(create(&frame_draw)?);
        let geometry = geometry.grid;
        let mut placement = drawing::instance(geometry, anchor);
        placement.clip = Some(clip);
        placement.transform = transform;
        // Translate inside the portal's rotated coordinate frame.
        let (s, c) = frame.yaw.sin_cos();
        placement.transform.translation = (-24. * c - 48. * s, -3., 24. * s - 48. * c);
        let draw = create(&placement)?;
        handles.push(draw);
        Ok(Scene {
            session: frame.session,
            dimension: frame.dimension,
            grid: geometry,
            placement,
            draw: Some(draw),
            handles: Vec::new(),
        })
    })();
    match result {
        Ok(mut scene) => {
            scene.handles = handles;
            Ok(scene)
        }
        Err(error) => {
            for id in handles.into_iter().rev() {
                let _ = remove(id);
            }
            Err(error)
        }
    }
}

/// Uploads prepared words once the host grants an active world, without guest cell conversion.
fn upload_inputs(inputs: &Inputs) -> Result<GeometrySet, DrawError> {
    let mut handles = Vec::new();
    let result = (|| {
        let plane = create_geometry(&inputs.plane)?;
        handles.push(plane);
        let frame = create_geometry(&inputs.frame)?;
        handles.push(frame);
        let grid = create_geometry(&inputs.grid)?;
        handles.push(grid);
        Ok(GeometrySet { plane, frame, grid })
    })();
    if result.is_err() {
        for id in handles.into_iter().rev() {
            let _ = remove(id);
        }
    }
    result
}

impl PlayerMod for Portal {
    fn init() {
        let vertices = [(-2., 0., 0.), (2., 0., 0.), (2., 5., 0.), (-2., 5., 0.)]
            .into_iter()
            .map(|position| Vertex {
                position,
                color: (1., 1., 1., 1.),
                uv: (0., 0.),
                data: (0., 0., 0., 0.),
            })
            .collect();
        INPUTS.with(|slot| {
            *slot.borrow_mut() = Some(Inputs {
                plane: GeometrySpec::Triangles(Mesh {
                    vertices,
                    indices: vec![0, 1, 2, 0, 2, 3],
                }),
                frame: GeometrySpec::Blocks(frame_grid()),
                grid: GeometrySpec::Blocks(grid()),
            })
        });
        let _ = hud::set_label("Portal view: F8 toggles the contents");
    }
    fn frame() {
        let Ok(Some(frame)) = gameplay::read_frame() else {
            return;
        };
        SCENE.with(|state| {
            let mut state = state.borrow_mut();
            if state.as_ref().is_some_and(|scene| {
                scene.session != frame.session || scene.dimension != frame.dimension
            }) {
                *state = None;
                GEOMETRY.with(|slot| *slot.borrow_mut() = None);
            }
            if state.is_none() {
                let result = GEOMETRY.with(|slot| {
                    let mut geometry = slot.borrow_mut();
                    if geometry.is_none() {
                        *geometry = Some(INPUTS.with(|inputs| {
                            upload_inputs(inputs.borrow().as_ref().expect("initialized inputs"))
                        })?);
                    }
                    Ok::<_, DrawError>(())
                });
                if let Err(error) = result {
                    let _ = hud::set_label(&format!("Portal view: {}", error.detail));
                    return;
                }

                let result = GEOMETRY.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .map(|geometry| create_scene(&frame, geometry))
                });
                let Some(result) = result else {
                    return;
                };
                match result {
                    Ok(scene) => *state = Some(scene),
                    Err(error) => {
                        let _ = hud::set_label(&format!("Portal view: {}", error.detail));
                        return;
                    }
                }
            }
            let scene = state.as_mut().unwrap();
            if let Err(error) = geometry_status(scene.grid) {
                let _ = hud::set_label(&format!("Portal view: {:?}: {}", error.code, error.detail));
            }
            if input::demo_pressed() {
                if let Some(draw) = scene.draw.take() {
                    let _ = remove(draw);
                    scene.handles.retain(|id| {
                        (id.owner, id.generation, id.slot)
                            != (draw.owner, draw.generation, draw.slot)
                    });
                } else if let Ok(draw) = create(&scene.placement) {
                    scene.draw = Some(draw);
                    scene.handles.push(draw);
                }
            }
        });
    }
}
mod_api::export_player_mod!(Portal);
