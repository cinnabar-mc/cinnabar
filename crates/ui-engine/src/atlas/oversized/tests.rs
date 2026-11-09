use super::*;
use crate::atlas::ServerAtlas;
use crate::sources::ImageWork;

/// Encodes an oversized image that fills its fitted server page.
fn png(color: u8) -> Arc<[u8]> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(512, 512, image::Rgba([color, 2, 3, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner().into()
}

#[test]
fn edits_detach_sorted_metadata_and_preserve_payload_ownership() {
    let bytes = png(1);
    let mut current = OversizedTextures::default();
    current.insert("z", &bytes);
    let old = current.clone();
    let (_, retry) = test_alloc::measure(|| current.insert("z", &bytes));
    assert_eq!(retry.calls, 0);
    assert!(current.shares_storage(&old));
    current.insert("a", &bytes);
    assert!(!current.shares_storage(&old));
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].0, "z");
    assert_eq!(
        current
            .iter()
            .map(|entry| entry.0.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert!(Arc::ptr_eq(&current[0].1, &bytes));
    let equal_bytes: Arc<[u8]> = bytes.as_ref().into();
    let previous = current.clone();
    current.insert("z", &equal_bytes);
    assert!(
        !current.same(&previous),
        "equal pixels with a different owner remain a source change"
    );
    assert!(Arc::ptr_eq(&previous[1].1, &bytes));
    assert!(Arc::ptr_eq(&current[1].1, &equal_bytes));
    current.insert("z", &png(8));
    assert_ne!(current[1].1, previous[1].1);
}

#[test]
fn snapshots_bound_current_storage_and_do_not_retain_a_history() {
    let bytes = png(1);
    let mut current = OversizedTextures::default();
    for index in 0..MAX_OVERSIZED {
        current.insert(&format!("{index:03}"), &bytes);
    }
    let old = current.clone();
    let retired = Arc::downgrade(old.0.as_ref().unwrap());
    current.insert("overflow", &bytes);
    assert!(current.shares_storage(&old));
    current.insert("000", &png(7));
    assert_eq!(current.len(), MAX_OVERSIZED);
    assert!(Arc::ptr_eq(&old[0].1, &bytes));
    drop(old);
    assert!(retired.upgrade().is_none());
    assert!(current.0.as_ref().unwrap().capacity() <= MAX_OVERSIZED);
    let (_, allocations) = test_alloc::measure(|| {
        for _ in 0..100 {
            let snapshot = current.clone();
            assert!(snapshot.shares_storage(&current));
        }
    });
    assert_eq!(allocations.calls, 0);
    assert_eq!(allocations.bytes, 0);
}

#[test]
fn fallback_owner_replacement_clears_sources_but_reinstalling_an_owner_does_not() {
    use crate::atlas::tests::MemoryPack;
    let first = Arc::new(MemoryPack::default());
    first.write("textures/art.png", png(1).to_vec());
    let second = Arc::new(MemoryPack::default());
    second.write("textures/art.png", png(9).to_vec());
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_image_work(ImageWork::inline())
        .with_fallbacks(Some(first.clone()), None);
    atlas.require(["textures/art"]);
    let old = atlas.oversized();
    atlas = atlas.with_fallbacks(Some(first), None);
    assert!(atlas.oversized().shares_storage(&old));
    atlas = atlas.with_fallbacks(Some(second), None);
    assert!(atlas.oversized().is_empty());
    assert!(atlas.placement("textures/art").is_none());
    atlas.require(["textures/art"]);
    let replacement = atlas.oversized();
    assert!(!replacement.same(&old));
    assert_ne!(replacement[0].1, old[0].1);
}

/// Encodes a minimal carrier retaining one original image outside its sprite atlas.
fn carrier(bytes: &[u8]) -> Arc<assets::RuntimeUiAssets> {
    let file = assets::UiFile {
        path: "textures/art.png".into(),
        bytes: bytes.into(),
    };
    let encoded = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[file]).unwrap();
    Arc::new(assets::RuntimeUiAssets::decode(&encoded).unwrap())
}

#[test]
fn carrier_and_builtin_replacements_publish_new_source_ownership() {
    let first = carrier(&png(1));
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_image_work(ImageWork::inline())
        .with_carrier(Some(first.clone()));
    atlas.require(["textures/art"]);
    let old = atlas.oversized();
    atlas = atlas.with_carrier(Some(first));
    assert!(atlas.oversized().shares_storage(&old));
    atlas = atlas.with_carrier(Some(carrier(&png(7))));
    assert!(atlas.oversized().is_empty());
    atlas.require(["textures/art"]);
    assert!(!atlas.oversized().same(&old));
    let bytes: &'static [u8] = Vec::leak(png(9).to_vec());
    let builtin: &'static [(&str, &[u8])] = Vec::leak(vec![("textures/art", bytes)]);
    atlas = atlas.with_builtin_images(builtin);
    assert!(atlas.oversized().is_empty());
    atlas.require(["textures/art"]);
    let shipped = atlas.oversized();
    assert_ne!(shipped[0].1, old[0].1);
    atlas = atlas.with_builtin_images(builtin);
    assert!(atlas.oversized().shares_storage(&shipped));
}

/// An atlas reading `count` distinct oversized pack textures, with their keys.
fn icon_atlas(count: usize) -> (ServerAtlas, Vec<String>) {
    use crate::atlas::tests::MemoryPack;
    let pack = Arc::new(MemoryPack::default());
    let keys: Vec<String> = (0..count)
        .map(|index| format!("textures/icon{index:03}"))
        .collect();
    for key in &keys {
        pack.write(&format!("{key}.png"), png(1).to_vec());
    }
    let atlas = ServerAtlas::new(&[], None, 1)
        .with_image_work(ImageWork::inline())
        .with_fallbacks(Some(pack), None);
    (atlas, keys)
}

/// Keys the atlas currently offers to the art pages.
fn offered(atlas: &ServerAtlas) -> Vec<String> {
    atlas
        .oversized()
        .iter()
        .map(|entry| entry.0.clone())
        .collect()
}

/// A menu scrolled past more large icons than the registry holds keeps offering the ones on
/// screen, replacing icons no frame draws any more.
#[test]
fn scrolling_offers_newly_drawn_textures_in_place_of_ones_left_behind() {
    let (mut atlas, keys) = icon_atlas(MAX_OVERSIZED + 3);
    let (first, rest) = keys.split_at(MAX_OVERSIZED);
    atlas.require(first.iter().map(String::as_str));
    atlas.end_oversized_frame(&BTreeSet::new(), |_| true);
    let scrolled: Vec<&str> = first[3..].iter().chain(rest).map(String::as_str).collect();
    atlas.require(scrolled.iter().copied());
    let now = offered(&atlas);
    assert_eq!(now.len(), MAX_OVERSIZED);
    for key in &scrolled {
        assert!(
            now.iter().any(|offered| offered == key),
            "{key} drawn but not offered"
        );
    }
}

/// A frame that draws more large textures than the registry holds never replaces one it
/// already drew.
#[test]
fn one_frame_never_replaces_its_own_textures() {
    let (mut atlas, keys) = icon_atlas(MAX_OVERSIZED + 1);
    atlas.require(keys.iter().map(String::as_str));
    let now = offered(&atlas);
    assert_eq!(now.len(), MAX_OVERSIZED);
    assert_eq!(now[..], keys[..MAX_OVERSIZED]);
}

/// Scrolling back to icons the registry still holds reports them as drawn again, and which
/// lack art, so the art pages can bring back the ones they dropped.
#[test]
fn scrolling_back_reports_the_retained_textures_drawn_without_art() {
    let (mut atlas, keys) = icon_atlas(MAX_OVERSIZED);
    let (top, bottom) = keys.split_at(MAX_OVERSIZED / 2);
    let bottom_set: BTreeSet<String> = bottom.iter().cloned().collect();
    for frame in [top, bottom] {
        atlas.require(frame.iter().map(String::as_str));
        atlas.end_oversized_frame(&BTreeSet::new(), |_| true);
    }
    atlas.require(top.iter().map(String::as_str));
    let ended = atlas.end_oversized_frame(&bottom_set, |key| bottom_set.contains(key));
    let drawn = ended.changed.expect("the drawn set changed");
    assert!(drawn.iter().eq(top.iter()));
    assert!(ended.uncovered, "the top icons have no art");
    assert_eq!(offered(&atlas), keys, "nothing was replaced");
    assert_eq!(
        atlas.end_oversized_frame(&drawn, |_| false),
        OversizedFrame {
            changed: Some(BTreeSet::new()),
            uncovered: false,
        },
        "a frame that draws nothing has nothing uncovered"
    );
}

/// Repeated frames report no change, while still telling whether a drawn texture lacks art.
#[test]
fn repeated_frames_report_coverage_without_a_change() {
    let (mut atlas, keys) = icon_atlas(3);
    let all: BTreeSet<String> = keys.iter().cloned().collect();
    atlas.require(keys.iter().map(String::as_str));
    assert_eq!(
        atlas.end_oversized_frame(&BTreeSet::new(), |_| true),
        OversizedFrame {
            changed: Some(all.clone()),
            uncovered: false,
        }
    );
    atlas.require(keys.iter().map(String::as_str));
    assert_eq!(
        atlas.end_oversized_frame(&all, |key| key != keys[1]),
        OversizedFrame {
            changed: None,
            uncovered: true,
        },
        "art dropped by a later atlas shows up without a change in what was drawn"
    );
}

/// A texture drawn straight from its copy counts as drawn for the frame.
#[test]
fn art_draws_count_as_drawn() {
    let (mut atlas, keys) = icon_atlas(1);
    atlas.require([keys[0].as_str()]);
    let drawn: BTreeSet<String> = keys.iter().cloned().collect();
    atlas.end_oversized_frame(&BTreeSet::new(), |_| true);
    atlas.draw_art(&keys[0]);
    assert_eq!(
        atlas.end_oversized_frame(&drawn, |_| true),
        OversizedFrame::default()
    );
}
