use super::*;

fn quad(uvs: [[u16; 2]; 4]) -> ModelQuad {
    ModelQuad {
        positions: [[0; 3]; 4],
        uvs,
        material: 0,
        flags: 0,
    }
}

#[test]
fn a_quad_samples_the_whole_texels_it_touches_in_any_corner_order() {
    let reversed = quad([[1024, 512], [1024, 448], [768, 448], [768, 512]]);
    assert_eq!(
        quad_region(&reversed, 64),
        Region {
            x: 12,
            y: 7,
            width: 4,
            height: 1,
        }
    );
    let partial = quad([[100, 100], [100, 100], [130, 100], [130, 100]]);
    assert_eq!(
        quad_region(&partial, 64),
        Region {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        }
    );
}

#[test]
fn regions_never_overlap_and_oversized_regions_are_refused() {
    let regions = (0..12)
        .map(|index| Region {
            x: index * 16,
            y: 0,
            width: 16 - index,
            height: 3 + index % 5,
        })
        .collect::<BTreeSet<_>>();
    let slots = pack_regions(&regions).unwrap();
    let placed = slots
        .iter()
        .map(|(&region, slot)| (region, footprint(region), slot))
        .collect::<Vec<_>>();
    for (index, (region, (width, height), slot)) in placed.iter().enumerate() {
        assert_eq!((slot.x % MIP_CELL, slot.y % MIP_CELL), (0, 0));
        assert!(slot.x + width <= 16 && slot.y + height <= 16);
        for (other, (other_width, other_height), other_slot) in &placed[index + 1..] {
            assert!(
                slot.tile != other_slot.tile
                    || slot.x + width <= other_slot.x
                    || other_slot.x + other_width <= slot.x
                    || slot.y + height <= other_slot.y
                    || other_slot.y + other_height <= slot.y,
                "{region:?} overlaps {other:?}"
            );
        }
    }
    let oversized = BTreeSet::from([Region {
        x: 0,
        y: 0,
        width: 17,
        height: 1,
    }]);
    assert!(pack_regions(&oversized).is_none());
}

/// Packs one-row regions of two opaque colors and returns their tile's mip chain.
fn thin_region_mips() -> Box<[assets::TextureMip]> {
    let side = 32;
    let rgba8 = (0..side * side)
        .flat_map(|index| {
            if index / side == 0 {
                [200, 40, 40, 255]
            } else {
                [40, 40, 200, 255]
            }
        })
        .collect::<Vec<_>>();
    let sheet = DecodedTexture {
        width: side,
        height: side,
        rgba8: rgba8.into_boxed_slice(),
    };
    let regions = BTreeSet::from([
        Region {
            x: 0,
            y: 0,
            width: 8,
            height: 1,
        },
        Region {
            x: 0,
            y: 8,
            width: 8,
            height: 1,
        },
    ]);
    let slots = pack_regions(&regions).unwrap();
    let mut tiles = tile_pixels(&sheet, &slots);
    assert_eq!(tiles.len(), 1);
    build_texture_mip_chain(tiles.remove(0)).unwrap()
}

#[test]
fn thin_regions_stay_covered_and_unmixed_while_minified() {
    let mips = thin_region_mips();
    for (level, mip) in mips.iter().enumerate().take(3).skip(1) {
        for (slot_x, color) in [(0, [200, 40, 40]), (8, [40, 40, 200])] {
            let start = ((slot_x >> level) * 4) as usize;
            let texel = &mip.rgba8[start..start + 4];
            assert_eq!(texel[..3], color, "level {level} slot {slot_x}");
            assert!(
                texel[3] >= assets::texture::ALPHA_TEST_THRESHOLD,
                "level {level} slot {slot_x} lost coverage: {texel:?}"
            );
        }
    }
}

#[test]
fn an_empty_span_on_the_far_edge_keeps_the_whole_sheet() {
    let sheet = DecodedTexture {
        width: 32,
        height: 32,
        rgba8: vec![255; 32 * 32 * 4].into_boxed_slice(),
    };
    let art = BTreeMap::from([(1, &sheet)]);
    let mut materials = vec![Material::unvaried(); 2];
    let base = [10, 20, 30, 255].repeat((TILE_SIZE * TILE_SIZE) as usize);
    let chain = assets::build_texture_mip_chain(base.into_boxed_slice(), TILE_SIZE).unwrap();
    let mut pages = vec![TexturePage::new(TextureArray {
        layers: 1,
        mips: chain,
    })];
    let visuals = [BlockVisual {
        kind: VisualKind::Model,
        model_template: 0,
        ..BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary)
    }];
    let template = |quad_start| ModelTemplate {
        quad_start,
        quad_count: 1,
        flags: 0,
    };
    let mut templates = [template(0), template(1)];
    let drawn = ModelQuad {
        material: 1,
        ..quad([[0, 4096], [0, 4096], [512, 4096], [512, 4096]])
    };
    let mut quads = [drawn; 2];
    install(
        &art,
        &mut materials,
        &mut pages,
        &visuals,
        &mut templates,
        &mut quads,
    )
    .unwrap();
    assert_eq!(templates[0].flags, 0);
    assert_eq!(quads[0], drawn);
}

#[test]
fn local_uvs_keep_their_offset_within_the_region() {
    assert_eq!(local_uv(1600, 64, 25, 0), 0);
    assert_eq!(local_uv(1664, 64, 25, 3), 1024);
    assert_eq!(local_uv(2624, 64, 25, 0), 4096);
}

#[test]
fn a_full_carrier_keeps_models_on_the_whole_sheet() {
    let sheet = DecodedTexture {
        width: 32,
        height: 32,
        rgba8: vec![255; 32 * 32 * 4].into_boxed_slice(),
    };
    let art = BTreeMap::from([(1, &sheet)]);
    let mut materials = vec![Material::unvaried(); 2];
    let mut pages = vec![TexturePage::new(TextureArray {
        layers: MAX_CARRIER_TEXTURE_LAYERS as u32,
        mips: Box::default(),
    })];
    let visuals = [BlockVisual {
        kind: VisualKind::Model,
        model_template: 0,
        ..BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary)
    }];
    let template = |quad_start| ModelTemplate {
        quad_start,
        quad_count: 1,
        flags: 0,
    };
    let mut templates = [template(0), template(1)];
    let drawn = ModelQuad {
        material: 1,
        ..quad([[0, 0], [0, 512], [512, 512], [512, 0]])
    };
    let mut quads = [drawn; 2];
    install(
        &art,
        &mut materials,
        &mut pages,
        &visuals,
        &mut templates,
        &mut quads,
    )
    .unwrap();
    assert_eq!(materials.len(), 2);
    assert_eq!(templates[0].flags, 0);
    assert_eq!(quads[0], drawn);
}
