use super::*;
use crate::BlockFace;
use crate::gui_item::cube_face;

/// One template with `flags` over no quads.
fn template(flags: u32) -> [ModelTemplate; 1] {
    [ModelTemplate {
        quad_start: 0,
        quad_count: 0,
        flags,
    }]
}

#[test]
fn world_shading_lights_each_outward_direction_like_flat_terrain() {
    for (face, expected) in [
        (BlockFace::Up, WORLD_SHADE_UP),
        (BlockFace::Down, WORLD_SHADE_DOWN),
        (BlockFace::North, WORLD_SHADE_Z),
        (BlockFace::South, WORLD_SHADE_Z),
        (BlockFace::West, WORLD_SHADE_X),
        (BlockFace::East, WORLD_SHADE_X),
    ] {
        assert_eq!(
            GuiShading::World.brightness(cube_face(face).0),
            expected,
            "{face:?}"
        );
    }
    // The visible inventory faces keep the cube's own brightness without the flag.
    for (face, _, _, brightness) in super::super::CUBE_FACES {
        assert_eq!(
            GuiShading::Inventory.brightness(cube_face(face).0),
            brightness
        );
    }
}

#[test]
fn only_flagged_model_templates_take_world_shading() {
    let flagged = template(MODEL_TEMPLATE_FLAG_GUI_WORLD_SHADE);
    let plain = template(0);
    assert_eq!(
        GuiShading::of(VisualKind::Model, Some(0), &flagged),
        GuiShading::World
    );
    assert_eq!(
        GuiShading::of(VisualKind::Model, Some(0), &plain),
        GuiShading::Inventory
    );
    assert_eq!(
        GuiShading::of(VisualKind::Model, None, &flagged),
        GuiShading::Inventory
    );
    assert_eq!(
        GuiShading::of(VisualKind::Cube, Some(0), &flagged),
        GuiShading::Inventory
    );
}
