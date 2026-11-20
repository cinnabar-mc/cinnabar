use super::*;
use std::collections::HashSet;

#[test]
fn shipped_carrier_packs_masks_animations_and_loading_frames_without_game_files() {
    let still = |key: &str, side: u32, alpha: u8| assets::oreui_icons::OreUiIcon {
        key: key.into(),
        width: side,
        height: side,
        frames: vec![assets::oreui_icons::OreUiIconFrame {
            millis: 0,
            rgba8: [30, 60, 90, alpha].repeat((side * side) as usize).into(),
        }],
    };
    let animation = |key: &str| assets::oreui_icons::OreUiIcon {
        key: key.into(),
        width: 2,
        height: 2,
        frames: (0..3)
            .map(|index| assets::oreui_icons::OreUiIconFrame {
                millis: 100 + index,
                rgba8: [index as u8; 16].into(),
            })
            .collect(),
    };
    let sleep = format!("{SLEEP_ANIMATION_PREFIX}one.gif");
    let manifest =
        assets::canonical_source_manifest_sha256(assets::oreui_icons::OREUI_ICONS_SOURCE_MANIFEST);
    let bytes = assets::oreui_icons::encode_oreui_icons(
        manifest,
        &[
            still(CHEVRON_LEFT_IMAGE, 3, 77),
            still(HARDCORE_ICON, 4, 255),
            animation(LOADING_ANIMATION),
            animation(&sleep),
        ],
    )
    .unwrap();
    let images = pack(shipped::decode(&bytes).unwrap()).unwrap();
    let texel = |key: &str| {
        let sprite = images.sprites[key];
        let page = &images.pages[usize::from(sprite.page)];
        let start = (usize::from(sprite.bounds[1]) * page.dimensions[0] as usize
            + usize::from(sprite.bounds[0]))
            * 4;
        <[u8; 4]>::try_from(&page.pixels[start..start + 4]).unwrap()
    };
    assert_eq!(texel(CHEVRON_LEFT_IMAGE), [30, 60, 90, 77]);
    assert_eq!(
        texel(&format!("@mask/{CHEVRON_LEFT_IMAGE}")),
        [255, 255, 255, 77]
    );
    assert!(
        !images
            .sprites
            .contains_key(&format!("@mask/{HARDCORE_ICON}"))
    );
    let millis: Vec<_> = images.loading_frames.iter().map(|(_, ms)| *ms).collect();
    assert_eq!(millis, [100, 101, 102]);
    assert_eq!(texel(&images.loading_frames[2].0), [2; 4]);
    assert_eq!(images.animations[&sleep].len(), 3);
    assert_eq!(texel(&sleep), texel(&format!("{sleep}#0")));
    let mut corrupt = bytes;
    corrupt[20] ^= 1;
    assert!(shipped::decode(&corrupt).is_err());
}

/// Every key a screen draws must ship, so no art is ever read from an installed game.
#[test]
fn every_referenced_key_ships_in_the_source_manifest() {
    let manifest: serde_json::Value =
        serde_json::from_slice(assets::oreui_icons::OREUI_ICONS_SOURCE_MANIFEST).unwrap();
    let shipped: HashSet<&str> = manifest["images"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|image| image["key"].as_str())
        .collect();
    for key in referenced_keys() {
        assert!(
            shipped.contains(key),
            "referenced art is not shipped: {key}"
        );
    }
}

/// A shelf page keeps only the rows and columns its images cover, so a handful of icons does
/// not hold a whole page-sized raster on the CPU and the GPU.
#[test]
fn packed_pages_are_cropped_to_the_images_they_hold() {
    let still =
        |side: u32, value: u8| Art::Still(side, side, vec![value; (side * side * 4) as usize]);
    let images = pack(vec![
        ("assets/a.png".to_owned(), still(3, 10)),
        ("assets/b.png".to_owned(), still(5, 20)),
    ])
    .unwrap();
    assert_eq!(images.pages.len(), 1);
    let page = &images.pages[0];
    // Three texels, one gutter column, then five: nine columns by five rows.
    assert_eq!(page.dimensions, [9, 5]);
    assert_eq!(page.pixels.len(), 9 * 5 * 4);
    for (key, value) in [("assets/a.png", 10), ("assets/b.png", 20)] {
        let [left, top, right, bottom] = images.sprites[key].bounds.map(usize::from);
        for y in top..bottom {
            for x in left..right {
                let start = (y * page.dimensions[0] as usize + x) * 4;
                assert_eq!(
                    page.pixels[start..start + 4],
                    [value; 4],
                    "{key} at {x},{y}"
                );
            }
        }
    }
}
