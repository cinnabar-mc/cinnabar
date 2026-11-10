use super::*;
use crate::block_entity::{
    MIN_DYNAMIC_ATLAS_WIDTH,
    atlas::{MAP_CELL, TEXT_CELL},
};

/// A diagnostic atlas wide enough for the runtime canvas rows, without installed game assets.
fn scene() -> BlockEntityScene {
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        MIN_DYNAMIC_ATLAS_WIDTH,
        8,
        &vec![255; MIN_DYNAMIC_ATLAS_WIDTH as usize * 8 * 4],
        &[],
    )
    .unwrap();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap());
    scene
}

/// A distinct opaque map canvas, so slot reuse cannot hide a wrong image.
fn map_canvas(key: u64) -> Vec<u8> {
    [key as u8 + 1, 19, 73, 255].repeat((MAP_CELL[0] * MAP_CELL[1]) as usize)
}

/// One text canvas with a recognizable color, independent of the font rasterizer.
fn text_canvas() -> Vec<u8> {
    [200, 100, 50, 255].repeat((TEXT_CELL[0] * TEXT_CELL[1]) as usize)
}

#[test]
fn a_twenty_four_map_wall_keeps_every_image_when_scan_order_changes() {
    let mut scene = scene();
    let mut rects = [None; 24];
    let text = scene.text_rect(1, text_canvas).unwrap();
    let mut settled_revision = None;
    for pass in 0..6 {
        assert_eq!(scene.text_rect(1, || panic!("retained text")), Some(text));
        for index in 0..rects.len() {
            let key = if pass % 2 == 0 {
                index
            } else {
                rects.len() - 1 - index
            };
            let rect = scene.map_rect(key as u64, || map_canvas(key as u64));
            if pass >= 2 {
                assert!(rect.is_some(), "map {key} is missing after settling");
                assert_eq!(
                    rect, rects[key],
                    "changing scan order cannot move a live map"
                );
            }
            rects[key] = rect;
        }
        let atlas = scene.atlas().unwrap();
        assert_eq!(
            atlas.text_cell(0),
            Some(text),
            "map growth must not move text"
        );
        let static_height = atlas.static_height() as f32;
        let width = atlas.size()[0] as usize;
        let frame = scene.update(SceneClock::default(), &[], &[]);
        for (key, rect) in rects.iter().enumerate() {
            let Some(rect) = rect else { continue };
            let at = (((rect.y - static_height) as usize) * width + rect.x as usize) * 4;
            assert_eq!(
                &frame.dynamic_rgba8[at..at + 4],
                &[key as u8 + 1, 19, 73, 255]
            );
        }
        if pass >= 2 {
            if let Some(revision) = settled_revision {
                assert_eq!(
                    frame.dynamic_revision, revision,
                    "stable maps do not repaint"
                );
            }
            settled_revision = Some(frame.dynamic_revision);
        }
    }
}

#[test]
fn growing_text_after_maps_preserves_both_pools_pixel_rects() {
    let mut scene = scene();
    let first_text = scene.text_rect(0, text_canvas).unwrap();
    let mut last_map = None;
    for _ in 0..3 {
        for key in 0..24 {
            last_map = scene.map_rect(key, || map_canvas(key));
        }
        scene.update(SceneClock::default(), &[], &[]);
    }
    let last_map = last_map.expect("the second map page is present");
    let mut last_text = None;
    for _ in 0..8 {
        for key in 0..24 {
            let map = scene.map_rect(key, || panic!("retained map"));
            if key == 23 {
                assert_eq!(map, Some(last_map));
            }
        }
        for key in 0..=100 {
            last_text = scene.text_rect(key, text_canvas);
        }
        scene.update(SceneClock::default(), &[], &[]);
    }
    assert!(
        last_text.is_some(),
        "text can grow after an appended map page"
    );
    assert_eq!(
        scene.text_rect(0, || panic!("retained text")),
        Some(first_text)
    );
    assert_eq!(
        scene.map_rect(23, || panic!("retained map")),
        Some(last_map)
    );
    assert_eq!(scene.atlas().unwrap().text_cell(0), Some(first_text));
}
