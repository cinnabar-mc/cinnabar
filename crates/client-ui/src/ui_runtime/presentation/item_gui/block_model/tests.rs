use assets::BlockFace;
use assets::gui_item::{GuiBlockQuad, cube_face};

use {super::*, assets::gui_item::GUI_ITEM_SIDE, ui::IconRef};

/// Places a small test tile on the requested atlas page.
fn tile(page: u16) -> IconRef {
    IconRef {
        color_mask: None,
        page,
        uv: [16, 32, 32, 48],
        glint: false,
    }
}

/// Makes a textured cube face for projection and batching assertions.
fn quad(face: BlockFace) -> GuiBlockQuad {
    let (corners, uvs) = cube_face(face);
    GuiBlockQuad {
        corners,
        uvs,
        material: 0,
        two_sided: false,
    }
}

#[test]
fn quads_paint_in_supplied_order_through_the_cube_projection() {
    // The caller supplies the paint order of the overlapping top faces.
    let mut slab_top = quad(BlockFace::Up);
    for corner in &mut slab_top.corners {
        corner[1] = 0.5;
    }
    let mut lower_top = quad(BlockFace::Up);
    for corner in &mut lower_top.corners {
        corner[1] = 0.0;
    }
    let quads = [(lower_top, tile(3)), (slab_top, tile(3))];
    let mesh = mesh(&quads, Default::default(), BlockModelOrder::Paint).unwrap();
    assert_eq!(mesh.vertices().len(), 8);
    assert_eq!(mesh.batches().len(), 1);
    let batch = &mesh.batches()[0];
    assert!(!batch.depth_test && !batch.depth_write);
    assert_eq!(batch.alpha_cutoff, Some(ALPHA_CUTOFF));
    for (vertex, corner) in mesh
        .vertices()
        .iter()
        .zip(quads.iter().flat_map(|(quad, _)| quad.corners))
    {
        let [x, y, _] = project_cube(corner);
        assert_eq!(vertex.position, [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE]);
        assert!(vertex.alpha_test);
    }
    // UVs land inside the quad's atlas tile, in texels.
    assert!(mesh.vertices().iter().all(|vertex| {
        (16.0..=32.0).contains(&vertex.uv[0]) && (32.0..=48.0).contains(&vertex.uv[1])
    }));
}

#[test]
fn overlapping_quads_preserve_paint_order_across_texture_pages() {
    let quads = [
        (quad(BlockFace::Up), tile(4)),
        (quad(BlockFace::South), tile(2)),
        (quad(BlockFace::West), tile(4)),
    ];
    let mesh = mesh(&quads, Default::default(), BlockModelOrder::Paint).unwrap();
    let pages: Vec<_> = mesh
        .batches()
        .iter()
        .map(|batch| (batch.texture_page, batch.index_range.len()))
        .collect();
    assert_eq!(pages, [(4, 6), (2, 6), (4, 6)]);
}

#[test]
fn a_uv_outside_its_tile_keeps_the_thumbnail() {
    let mut wrapped = quad(BlockFace::Up);
    wrapped.uvs[2] = [1.5, 1.0];
    assert!(
        mesh(
            &[(wrapped, tile(1))],
            Default::default(),
            BlockModelOrder::Paint
        )
        .is_none()
    );
}

#[test]
fn rear_faces_do_not_fill_cutout_holes() {
    let mesh = mesh(
        &[
            (quad(BlockFace::Up), tile(1)),
            (quad(BlockFace::Down), tile(1)),
        ],
        Default::default(),
        BlockModelOrder::Paint,
    )
    .unwrap();
    assert_eq!(mesh.vertices().len(), 4, "only the outward top face draws");
}

#[test]
fn authored_two_sided_planes_remain_visible_from_the_back() {
    let mut plane = quad(BlockFace::Down);
    plane.two_sided = true;
    assert_eq!(
        mesh(
            &[(plane, tile(1))],
            Default::default(),
            BlockModelOrder::Paint
        )
        .unwrap()
        .vertices()
        .len(),
        4
    );
}

#[test]
fn depth_order_keeps_overlapping_surfaces_and_groups_texture_pages() {
    let mut top = quad(BlockFace::Up);
    let mut lower = top;
    for corner in &mut lower.corners {
        corner[1] = 0.0;
    }
    for corner in &mut top.corners {
        corner[1] = 0.5;
    }
    let quads = [(lower, tile(4)), (top, tile(2)), (top, tile(4))];
    let mesh = mesh(&quads, Default::default(), BlockModelOrder::Depth).unwrap();
    assert!(mesh.batches().iter().all(|b| b.depth_test && b.depth_write));
    let pages: Vec<_> = mesh
        .batches()
        .iter()
        .map(|b| (b.texture_page, b.index_range.len()))
        .collect();
    assert_eq!(pages, [(2, 6), (4, 12)]);
    let nearest = |range: std::ops::Range<usize>| {
        mesh.vertices()[range]
            .iter()
            .map(|v| v.clip_z)
            .fold(f32::MIN, f32::max)
    };
    assert!(nearest(4..8) > nearest(0..4));
}
