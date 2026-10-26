use super::*;
use crate::{MATERIAL_FLAG_ISOTROPIC, MATERIAL_FLAG_ROTATE_UV, TextureMip, TextureRef};

#[test]
fn azalea_inventory_has_a_separate_stem_and_open_bottom_canopy() {
    let source: Vec<_> = BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (corners, uvs) = cube_face(face);
            ModelQuad {
                positions: corners.map(|p| p.map(|v| (v * 256.0) as i16)),
                uvs: uvs.map(|p| p.map(|v| (v * 4096.0) as u16)),
                material: face as u32,
                flags: face.model_quad_face_id(),
            }
        })
        .collect();
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: source.len() as u32,
        flags: crate::MODEL_TEMPLATE_FLAG_AZALEA,
    }];
    let faces = BlockFace::ALL.map(|face| face as u32);
    let held = block_item_quads(VisualKind::Model, faces, Some(0), &templates, &source).unwrap();
    let inventory = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        faces,
        Some(0),
        &templates,
        &source,
    )
    .unwrap();
    for face in BlockFace::ALL
        .into_iter()
        .filter(|face| *face != BlockFace::Down)
    {
        let (corners, uvs) = cube_face(face);
        let canopy = inventory
            .iter()
            .find(|quad| quad.corners == corners)
            .unwrap();
        assert_eq!(
            canopy.uvs, uvs,
            "{face:?}: canopy preserves directional artwork"
        );
    }
    assert_eq!(held.len(), 6);
    assert_eq!(inventory.len(), 7);
    assert!(inventory.iter().all(|quad| quad.two_sided));
    let stems: Vec<_> = inventory
        .iter()
        .filter(|quad| quad.material == faces[BlockFace::East as usize])
        .collect();
    assert_eq!(stems.len(), 2);
    for stem in stems {
        assert_eq!(
            stem.corners
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min),
            0.0
        );
        assert_eq!(
            stem.corners
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max),
            1.0
        );
    }
    assert!(
        !inventory
            .iter()
            .any(|quad| quad.corners.iter().all(|p| p[1] == 0.0))
    );
    assert_eq!(
        inventory
            .iter()
            .filter(|quad| quad.material == faces[BlockFace::North as usize])
            .count(),
        4
    );
    assert_eq!(
        inventory
            .iter()
            .filter(|quad| quad.material == faces[BlockFace::Up as usize])
            .count(),
        1
    );
}

#[test]
fn heavy_core_inventory_preview_is_enlarged_and_repositioned() {
    let source: Vec<_> = BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (corners, uvs) = cube_face(face);
            ModelQuad {
                positions: corners.map(|[x, y, z]| {
                    [
                        (64.0 + x * 128.0) as i16,
                        (y * 128.0) as i16,
                        (64.0 + z * 128.0) as i16,
                    ]
                }),
                uvs: uvs.map(|p| p.map(|v| (v * 2048.0) as u16)),
                material: 1,
                flags: face.model_quad_face_id(),
            }
        })
        .collect();
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: source.len() as u32,
        flags: crate::MODEL_TEMPLATE_FLAG_HEAVY_CORE,
    }];
    let held = block_item_quads(VisualKind::Model, [1; 6], Some(0), &templates, &source).unwrap();
    let inventory = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        [1; 6],
        Some(0),
        &templates,
        &source,
    )
    .unwrap();
    assert_eq!(inventory.len(), held.len());
    let (scale, offset) = HEAVY_CORE_GUI_TRANSFORM;
    for (held, inventory) in held.iter().zip(&inventory) {
        assert_eq!(held.uvs, inventory.uvs);
        for (placed, shown) in held.corners.into_iter().zip(inventory.corners) {
            let placed = super::super::project_cube(placed);
            let shown = super::super::project_cube(shown);
            let expected = [
                super::super::CUBE_OFFSET[0]
                    + (placed[0] - super::super::CUBE_OFFSET[0]) * scale
                    + offset[0],
                super::super::CUBE_OFFSET[1]
                    + (placed[1] - super::super::CUBE_OFFSET[1]) * scale
                    + offset[1],
                placed[2] * scale,
            ];
            for axis in 0..3 {
                assert!(
                    (shown[axis] - expected[axis]).abs() < 0.00001,
                    "axis {axis}: {shown:?} != {expected:?}"
                );
            }
        }
    }
}

#[test]
fn shelf_inventory_preview_is_enlarged_and_repositioned() {
    let source: Vec<_> = BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (corners, uvs) = cube_face(face);
            ModelQuad {
                positions: corners.map(|[x, y, z]| {
                    [
                        (64.0 + x * 128.0) as i16,
                        (y * 128.0) as i16,
                        (64.0 + z * 128.0) as i16,
                    ]
                }),
                uvs: uvs.map(|p| p.map(|v| (v * 2048.0) as u16)),
                material: 1,
                flags: face.model_quad_face_id(),
            }
        })
        .collect();
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: source.len() as u32,
        flags: crate::MODEL_TEMPLATE_FLAG_SHELF,
    }];
    let held = block_item_quads(VisualKind::Model, [1; 6], Some(0), &templates, &source).unwrap();
    let inventory = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        [1; 6],
        Some(0),
        &templates,
        &source,
    )
    .unwrap();
    assert_eq!(inventory.len(), held.len());
    let (scale, offset) = SHELF_GUI_TRANSFORM;
    for (held, inventory) in held.iter().zip(&inventory) {
        assert_eq!(held.uvs, inventory.uvs);
        for (placed, shown) in held.corners.into_iter().zip(inventory.corners) {
            let placed = super::super::project_cube(placed);
            let shown = super::super::project_cube(shown);
            let expected = [
                super::super::CUBE_OFFSET[0]
                    + (placed[0] - super::super::CUBE_OFFSET[0]) * scale
                    + offset[0],
                super::super::CUBE_OFFSET[1]
                    + (placed[1] - super::super::CUBE_OFFSET[1]) * scale
                    + offset[1],
                placed[2] * scale,
            ];
            for axis in 0..3 {
                assert!(
                    (shown[axis] - expected[axis]).abs() < 0.00001,
                    "axis {axis}: {shown:?} != {expected:?}"
                );
            }
        }
    }
}

#[test]
fn chorus_inventory_geometry_has_three_outward_protrusions() {
    let source: Vec<_> = BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (corners, uvs) = cube_face(face);
            ModelQuad {
                positions: corners.map(|p| p.map(|v| (v * 256.0) as i16)),
                uvs: uvs.map(|p| p.map(|v| (v * 4096.0) as u16)),
                material: 1,
                flags: face.model_quad_face_id(),
            }
        })
        .collect();
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: source.len() as u32,
        flags: crate::MODEL_TEMPLATE_FLAG_CHORUS,
    }];
    let inventory = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        [1; 6],
        Some(0),
        &templates,
        &source,
    )
    .unwrap();
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for point in inventory.iter().flat_map(|quad| quad.corners) {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    assert_eq!(bounds, [[-0.125, 0.0, 0.0], [1.0, 1.125, 1.125]]);
    for (axis, edge) in [(0, -0.125), (1, 1.125), (2, 1.125)] {
        assert!(
            inventory.iter().any(|quad| {
                quad.visible_in_gui() && quad.corners.iter().all(|p| p[axis] == edge)
            }),
            "missing visible protrusion along axis {axis}"
        );
    }
    assert!(
        inventory
            .iter()
            .flat_map(|quad| quad.uvs)
            .flatten()
            .all(|v| (0.0..=1.0).contains(&v))
    );
    let held = block_item_quads(VisualKind::Model, [1; 6], Some(0), &templates, &source).unwrap();
    assert!(
        held.iter()
            .flat_map(|quad| quad.corners)
            .flatten()
            .all(|v| (0.0..=1.0).contains(&v))
    );
}

#[test]
fn inventory_panes_show_a_full_plate_with_body_and_edge_materials() {
    let materials = BlockFace::ALL.map(|face| face as u32 + 1);
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: crate::MODEL_TEMPLATE_FLAG_PANE,
    }];
    let source = [ModelQuad {
        positions: [
            [112, 0, 112],
            [112, 0, 144],
            [112, 256, 144],
            [112, 256, 112],
        ],
        uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        material: materials[BlockFace::West as usize],
        flags: 0,
    }];
    let inventory = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        materials,
        Some(0),
        &templates,
        &source,
    )
    .unwrap();
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for point in inventory.iter().flat_map(|quad| quad.corners) {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    assert_eq!(
        bounds,
        [[7.0 / 16.0 - 0.2, 0.0, 0.0], [9.0 / 16.0 - 0.2, 1.0, 1.0]]
    );
    for face in [
        BlockFace::Up,
        BlockFace::Down,
        BlockFace::North,
        BlockFace::South,
    ] {
        let u = inventory[face as usize].uvs.map(|uv| uv[0]);
        assert_eq!(u.into_iter().fold(f32::INFINITY, f32::min), 7.0 / 16.0);
        assert_eq!(u.into_iter().fold(f32::NEG_INFINITY, f32::max), 9.0 / 16.0);
    }
    let visible: Vec<_> = inventory
        .iter()
        .filter(|quad| quad.visible_in_gui())
        .collect();
    assert_eq!(visible.len(), 3);
    assert_eq!(
        visible
            .iter()
            .filter(|quad| quad.material == materials[BlockFace::Up as usize])
            .count(),
        2
    );
    assert_eq!(
        visible
            .iter()
            .filter(|quad| quad.material == materials[BlockFace::West as usize])
            .count(),
        1
    );
    let held =
        block_item_quads(VisualKind::Model, materials, Some(0), &templates, &source).unwrap();
    assert_eq!(
        held.len(),
        source.len(),
        "held geometry keeps the placed template"
    );
}

#[test]
fn walls_use_two_carried_posts_with_a_lower_connecting_span() {
    let world_post = carried_cuboid_faces([64, 0, 64], [192, 256, 192]).map(|corners| ModelQuad {
        positions: corners.map(|point| point.map(|value| (value * 256.0) as i16)),
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 7,
        flags: 0,
    });
    let quads = block_item_quads(
        VisualKind::Model,
        [7; 6],
        Some(0),
        &[ModelTemplate {
            quad_start: 0,
            quad_count: world_post.len() as u32,
            flags: crate::MODEL_TEMPLATE_FLAG_WALL,
        }],
        &world_post,
    )
    .unwrap();
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for quad in &quads {
        assert_eq!(quad.material, 7);
        assert!(quad.uvs.iter().flatten().all(|uv| (0.0..=1.0).contains(uv)));
        for point in quad.corners {
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
    }
    assert_eq!(bounds, [[-0.25, 0.0, 0.25], [1.25, 1.0, 0.75]]);
    let top_faces = quads
        .iter()
        .filter(|quad| quad.corners.iter().all(|point| point[1] == 1.0))
        .collect::<Vec<_>>();
    assert_eq!(top_faces.len(), 2);
    assert!(
        top_faces
            .iter()
            .any(|quad| quad.corners.iter().all(|p| p[0] <= 0.25))
    );
    assert!(
        top_faces
            .iter()
            .any(|quad| quad.corners.iter().all(|p| p[0] >= 0.75))
    );
    assert!(quads.iter().any(|quad| {
        quad.corners.iter().all(|p| p[1] == 13.0 / 16.0)
            && quad.corners.iter().any(|p| p[0] < 0.5)
            && quad.corners.iter().any(|p| p[0] > 0.5)
    }));
}

#[test]
fn vanilla_gates_use_the_custom_carried_inventory_axis() {
    let quad = ModelQuad {
        positions: [[0, 0, 64], [256, 0, 64], [256, 256, 192], [0, 256, 192]],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 7,
        flags: 0,
    };
    let quads = block_item_quads(
        VisualKind::Model,
        [7; 6],
        Some(0),
        &[
            ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: crate::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
                    | crate::MODEL_TEMPLATE_FLAG_GATE_AXIS_Z,
            },
            ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            },
        ],
        &[quad],
    )
    .unwrap();
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for point in quads.iter().flat_map(|quad| quad.corners) {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    assert_eq!(bounds, [[0.25, 0.0, 0.0], [0.75, 1.0, 1.0]]);
}

#[test]
fn carried_walls_preserve_directional_materials() {
    let materials = BlockFace::ALL.map(|face| face as u32 + 1);
    for quad in carried_wall_quads(materials) {
        let [a, b, c, _] = quad.corners;
        let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
        let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let face = if normal[0] < 0.0 {
            BlockFace::West
        } else if normal[0] > 0.0 {
            BlockFace::East
        } else if normal[1] < 0.0 {
            BlockFace::Down
        } else if normal[1] > 0.0 {
            BlockFace::Up
        } else if normal[2] < 0.0 {
            BlockFace::North
        } else {
            BlockFace::South
        };
        assert_eq!(quad.material, materials[face as usize]);
    }
}

#[test]
fn cube_gui_surfaces_face_the_inventory_camera() {
    let faces = BlockFace::ALL.map(|face| face as u32 + 1);
    let quads = block_item_quads(VisualKind::Cube, faces, None, &[], &[]).unwrap();
    for quad in &quads {
        let [a, b, c, _] = quad.corners.map(super::super::project_cube);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        assert!(
            area < 0.0,
            "rear material {} is visible through the icon",
            quad.material
        );
    }
    assert_eq!(quads.len(), CUBE_FACES.len());
}

#[test]
fn gui_materials_preserve_tiles_and_alpha_modes() {
    let array = TextureArray {
        layers: 1,
        mips: vec![TextureMip {
            size: 1,
            rgba8: vec![20, 40, 60, 128].into(),
        }]
        .into(),
    };
    for alpha in [0, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT] {
        for flags in [
            MATERIAL_FLAG_ISOTROPIC,
            MATERIAL_FLAG_ISOTROPIC | MATERIAL_FLAG_DISABLE_AO,
        ] {
            let materials = [
                Material::unvaried(),
                Material {
                    texture: TextureRef::new(0, 0).unwrap(),
                    flags: alpha | flags,
                    ..Material::unvaried()
                },
            ];
            let (tile, side, blend) = material_tile(&materials, 1, |_| Some(&array))
                .expect("ambient occlusion does not affect isolated GUI tiles");
            assert_eq!(tile, [20, 40, 60, 128]);
            assert_eq!(side, 1);
            assert_eq!(blend, alpha == MATERIAL_FLAG_ALPHA_BLEND);
            let mut unsupported = materials;
            unsupported[1].flags |= MATERIAL_FLAG_ROTATE_UV;
            assert_eq!(
                material_tile(&unsupported, 1, |_| Some(&array)),
                Err(GuiBlockReject::Material)
            );
        }
    }
}

#[test]
fn fences_draw_the_native_two_post_inventory_shape() {
    let template = ModelTemplate {
        quad_start: 0,
        quad_count: 0,
        flags: MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    };
    let quads = block_item_quads(VisualKind::Model, [7; 6], Some(0), &[template], &[]).unwrap();
    assert_eq!(
        quads.len(),
        INVENTORY_FENCE_BOXES.len() * BlockFace::ALL.len()
    );
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for quad in &quads {
        assert_eq!(quad.material, 7);
        assert!(quad.uvs.iter().flatten().all(|uv| (0.0..=1.0).contains(uv)));
        for point in quad.corners {
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
    }
    // Posts span the block's Z edges and both rails overhang them by two pixels.
    assert_eq!(bounds, [[0.375, 0.0, -0.125], [0.625, 1.0, 1.125]]);
    let tops = quads
        .iter()
        .filter(|quad| quad.corners.iter().all(|point| point[1] == 1.0))
        .count();
    assert_eq!(tops, 2, "only the two posts reach the top");
    // Held fences draw one-sided, so faces hidden from the inventory camera remain.
    let post_east_faces = quads
        .iter()
        .filter(|quad| quad.corners.iter().all(|point| point[0] == 0.625))
        .count();
    assert_eq!(post_east_faces, 2);
}

#[test]
fn lighting_flags_preserve_material_tiles_without_admitting_unresolved_tint() {
    let array = TextureArray {
        layers: 1,
        mips: vec![crate::TextureMip {
            size: 1,
            rgba8: vec![20, 40, 60, 255].into(),
        }]
        .into(),
    };
    for flags in [
        MATERIAL_FLAG_ALPHA_CUTOUT | crate::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
        MATERIAL_FLAG_DISABLE_AO | crate::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
    ] {
        let materials = [
            Material::unvaried(),
            Material {
                flags,
                ..Material::unvaried()
            },
        ];
        let (pixels, side, blend) = material_tile(&materials, 1, |_| Some(&array)).unwrap();
        assert_eq!(pixels, &[20, 40, 60, 255]);
        assert_eq!(side, 1);
        assert!(!blend);
    }
    let materials = [
        Material::unvaried(),
        Material {
            flags: crate::MATERIAL_FLAG_GRASS_TINT,
            ..Material::unvaried()
        },
    ];
    assert_eq!(
        material_tile(&materials, 1, |_| Some(&array)),
        Err(GuiBlockReject::Material)
    );
}

/// Each visible rail half samples ten texture columns from its own start.
#[test]
fn gui_fence_rail_halves_restart_their_texture_without_changing_held_meshes() {
    for flags in [
        MODEL_TEMPLATE_FLAG_FENCE_WOOD,
        crate::MODEL_TEMPLATE_FLAG_FENCE_NETHER,
    ] {
        let templates = [ModelTemplate {
            quad_start: 0,
            quad_count: 0,
            flags,
        }];
        let gui = block_gui_quads(
            Default::default(),
            VisualKind::Model,
            [7; 6],
            Some(0),
            &templates,
            &[],
        )
        .unwrap();
        for height in [5.0 / 16.0, 13.0 / 16.0] {
            let front = gui
                .iter()
                .find(|q| {
                    q.corners.iter().all(|p| p[0] == 7.0 / 16.0)
                        && q.corners.iter().any(|p| p[1] == height)
                        && q.corners.iter().any(|p| p[2] == 18.0 / 16.0)
                })
                .unwrap();
            let end = front
                .corners
                .iter()
                .map(|p| p[2])
                .fold(f32::INFINITY, f32::min);
            assert_eq!(
                end, 0.5,
                "the front rail runs from its midpoint to the overhang"
            );
            for (corner, uv) in front.corners.iter().zip(front.uvs) {
                assert_eq!(uv, [corner[2] - 0.5, 1.0 - corner[1]]);
            }
            let back = gui
                .iter()
                .find(|q| {
                    q.corners.iter().all(|p| p[0] == 7.0 / 16.0)
                        && q.corners.iter().any(|p| p[1] == height)
                        && q.corners.iter().any(|p| p[2] == -2.0 / 16.0)
                })
                .unwrap();
            for (corner, uv) in back.corners.iter().zip(back.uvs) {
                assert_eq!(uv, [corner[2] + 2.0 / 16.0, 1.0 - corner[1]]);
            }
        }
        let held = block_item_quads(VisualKind::Model, [7; 6], Some(0), &templates, &[]).unwrap();
        assert_eq!(
            held.len(),
            INVENTORY_FENCE_BOXES.len() * BlockFace::ALL.len()
        );
        assert!(
            held.iter()
                .all(|q| q.uvs.iter().flatten().all(|uv| (0.0..=1.0).contains(uv)))
        );
    }
}

#[test]
fn bamboo_inventory_remains_independent_of_wooden_fence_uvs() {
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: 0,
        flags: MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    }];
    let gui = block_gui_quads(
        crate::facts::BlockRenderKind::BambooFence,
        VisualKind::Model,
        [7; 6],
        Some(0),
        &templates,
        &[],
    )
    .unwrap();
    let held = block_item_quads(VisualKind::Model, [7; 6], Some(0), &templates, &[]).unwrap();
    assert_eq!(gui, held);
    let ordinary = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        [7; 6],
        Some(0),
        &templates,
        &[],
    )
    .unwrap();
    assert_ne!(gui, ordinary);
}

#[test]
fn fence_side_diagonal_keeps_the_projected_lower_texel_row() {
    use glam::Vec2;
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: 0,
        flags: MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    }];
    let quads = block_gui_quads(
        Default::default(),
        VisualKind::Model,
        [7; 6],
        Some(0),
        &templates,
        &[],
    )
    .unwrap();
    let face = quads
        .iter()
        .find(|q| q.corners.iter().all(|p| p[2] == 0.25) && q.corners.iter().any(|p| p[1] == 1.0))
        .unwrap();
    // At this physical placement, half-precision XY has quarter-pixel spacing.
    let positions = face.corners.map(|corner| {
        let [x, y, _] = super::super::project_cube(corner);
        Vec2::new(
            ((419.0 + x) * 4.0).round() / 4.0 - 419.0,
            ((440.0 + y) * 4.0).round() / 4.0 - 440.0,
        )
    });
    let pixel = Vec2::new(6.5, 10.5);
    let uv = [[0, 1, 2], [0, 2, 3]]
        .into_iter()
        .find_map(|ids| {
            let [a, b, c] = ids.map(|i| positions[i]);
            let area = (b - a).perp_dot(c - a);
            let wb = (pixel - a).perp_dot(c - a) / area;
            let wc = (b - a).perp_dot(pixel - a) / area;
            let weights = [1.0 - wb - wc, wb, wc];
            if weights.iter().any(|w| *w < 0.0) {
                return None;
            }
            Some(
                ids.into_iter()
                    .zip(weights)
                    .fold(Vec2::ZERO, |uv, (i, w)| uv + Vec2::from(face.uvs[i]) * w),
            )
        })
        .unwrap();
    assert_eq!(
        (uv.y * 16.0).floor(),
        13.0,
        "the opposite diagonal samples the next texture row"
    );
}
