use super::*;
use world::{BLOCKS_PER_SUB_CHUNK, RawBlockIds};

const PORTAL: u32 = 7;
const ORDINARY: u32 = 1;

fn fixture(portal_position: Option<[usize; 3]>, secondary_portal: bool) -> SubChunk {
    let mut bytes = vec![8, if secondary_portal { 2 } else { 1 }, 3];
    let mut words = vec![0_u32; BLOCKS_PER_SUB_CHUNK / u32::BITS as usize];
    if let Some([x, y, z]) = portal_position {
        let linear = (x * SUB_CHUNK_SIDE + z) * SUB_CHUNK_SIDE + y;
        words[linear / u32::BITS as usize] |= 1 << (linear % u32::BITS as usize);
    }
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    // The deliberately unused palette entry must not create a surface.
    bytes.extend_from_slice(&[4, (ORDINARY * 2) as u8, (PORTAL * 2) as u8]);
    if secondary_portal {
        bytes.extend_from_slice(&[1, (PORTAL * 2) as u8]);
    }
    SubChunk::decode(&bytes, &RawBlockIds { air: 0 })
}

fn classify(id: u32) -> Option<BlockEntityKind> {
    (id == PORTAL).then_some(BlockEntityKind::EndPortal)
}

/// Collects one sub-chunk's cells and submits those near `eye`, as a frame does.
fn submit_sub_chunk(
    submissions: &mut Vec<BlockEntitySubmission>,
    grid: [i32; 3],
    sub_chunk: &SubChunk,
    eye: Vec3,
    classify: &mut impl FnMut(u32) -> Option<BlockEntityKind>,
) {
    submit(
        submissions,
        &sub_chunk_cells(grid, sub_chunk, classify),
        eye,
        1.0,
        |_| (0, 0),
    );
}

#[test]
fn palette_beds_use_both_halves_light_before_entity_nbt_arrives() {
    use render_model::{BedModel, BlockEntityLight};

    let block = [2, 3, 4];
    let center = block.map(|value| value as f32 + 0.5);
    for direction in 0..4 {
        let model = BedModel {
            color: render_model::bed_color(render_model::DEFAULT_BED_DYE).unwrap(),
            head: true,
            direction,
        };
        let offset = model.other_half_offset();
        let foot = std::array::from_fn(|axis| center[axis] + offset[axis] as f32);
        let cells = sub_chunk_cells([0; 3], &fixture(Some([2, 3, 4]), false), &mut |id| {
            (id == PORTAL).then_some(BlockEntityKind::Bed(model))
        });
        for (head_light, foot_light, expected) in [
            ((0, 0), (0, 0), BlockEntityLight::Actor { block: 0, sky: 0 }),
            (
                (2, 12),
                (11, 4),
                BlockEntityLight::Actor { block: 11, sky: 12 },
            ),
        ] {
            let mut submissions = Vec::new();
            let mut sampled = Vec::new();
            submit(
                &mut submissions,
                &cells,
                Vec3::from_array(center),
                1.0,
                |position| {
                    sampled.push(position);
                    if position == center {
                        head_light
                    } else {
                        foot_light
                    }
                },
            );
            assert_eq!(sampled, [center, foot]);
            assert_eq!(submissions.len(), 1);
            assert_eq!(submissions[0].block, block);
            assert_eq!(submissions[0].kind, BlockEntityKind::Bed(model));
            assert_eq!(submissions[0].light, expected);
        }
    }
}

#[test]
fn palette_submission_preserves_portal_and_piston_lighting() {
    use render_model::{BlockEntityLight, PistonArmModel};

    let block = [2, 3, 4];
    let piston = PistonArmModel {
        facing: 1,
        sticky: false,
        progress: 0.0,
    };
    let cells = vec![
        (block, BlockEntityKind::EndPortal),
        (block, BlockEntityKind::EndGateway),
        (block, BlockEntityKind::PistonArm(piston)),
    ];
    let mut submissions = Vec::new();
    let mut sampled = Vec::new();
    submit(&mut submissions, &cells, Vec3::ZERO, 0.0, |position| {
        sampled.push(position);
        (3, 7)
    });
    assert_eq!(sampled, [piston.plate_center(block)]);
    assert_eq!(submissions.len(), 3);
    assert_eq!(submissions[0].light, 1.0.into());
    assert_eq!(submissions[1].light, 1.0.into());
    assert_eq!(
        submissions[2].light,
        BlockEntityLight::Actor { block: 3, sky: 7 }
    );
}

#[test]
fn primary_palette_portals_are_drawn_without_block_entity_nbt() {
    let side = SUB_CHUNK_SIDE as i32;
    let block = [-side + 2, side + 3, -side + 4];
    let eye = Vec3::from_array(block.map(|value| value as f32 + 0.5));
    let mut submissions = Vec::new();
    submit_sub_chunk(
        &mut submissions,
        [-1, 1, -1],
        &fixture(Some([2, 3, 4]), false),
        eye,
        &mut classify,
    );
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].block, block);
    assert_eq!(submissions[0].kind, BlockEntityKind::EndPortal);
    assert_eq!(submissions[0].light, 1.0.into());
}

#[test]
fn unused_palette_entries_and_secondary_storage_cannot_create_portal_surfaces() {
    for secondary in [false, true] {
        let mut submissions = Vec::new();
        submit_sub_chunk(
            &mut submissions,
            [0; 3],
            &fixture(None, secondary),
            Vec3::ZERO,
            &mut classify,
        );
        assert!(submissions.is_empty());
    }
}

#[test]
fn portal_removal_immediately_removes_its_surface_and_preserves_submission_limits() {
    let portal = fixture(Some([0; 3]), false);
    let mut submissions = vec![
        BlockEntitySubmission {
            block: [0; 3],
            light: 1.0.into(),
            kind: BlockEntityKind::EndPortal,
        };
        super::super::MAX_SUBMISSIONS
    ];
    submit_sub_chunk(&mut submissions, [0; 3], &portal, Vec3::ZERO, &mut classify);
    assert_eq!(submissions.len(), super::super::MAX_SUBMISSIONS);
    submissions.clear();
    submit_sub_chunk(
        &mut submissions,
        [0; 3],
        &fixture(None, false),
        Vec3::ZERO,
        &mut classify,
    );
    assert!(submissions.is_empty());
    submit_sub_chunk(
        &mut submissions,
        [0; 3],
        &portal,
        Vec3::splat(super::super::SCAN_RADIUS_BLOCKS * 2.0),
        &mut classify,
    );
    assert!(submissions.is_empty());
}
