use super::*;
use crate::atlas::{MAX_OVERSIZED, ServerAtlas};
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

/// Scrolling a full grid of large icons replaces icons that left the screen, as The Hive's
/// game picker does when its last row comes into view.
#[test]
fn a_full_registry_replaces_textures_no_longer_drawn() {
    use crate::atlas::tests::MemoryPack;
    let pack = Arc::new(MemoryPack::default());
    let keys: Vec<String> = (0..MAX_OVERSIZED + 3)
        .map(|index| format!("textures/icon{index:03}"))
        .collect();
    for key in &keys {
        pack.write(&format!("{key}.png"), png(1).to_vec());
    }
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_image_work(ImageWork::inline())
        .with_fallbacks(Some(pack), None);
    let (first, rest) = keys.split_at(MAX_OVERSIZED);
    atlas.require(first.iter().map(String::as_str));
    assert_eq!(atlas.oversized().len(), MAX_OVERSIZED);
    let scrolled: Vec<&str> = first[3..].iter().chain(rest).map(String::as_str).collect();
    atlas.require(scrolled.iter().copied());
    let offered = atlas.oversized();
    let offered: Vec<&str> = offered.iter().map(|entry| entry.0.as_str()).collect();
    for key in &scrolled {
        assert!(offered.contains(key), "{key} drawn but not offered");
    }
    assert_eq!(offered.len(), MAX_OVERSIZED);
}

#[test]
fn a_frame_drawing_every_entry_keeps_them_all() {
    use crate::atlas::tests::MemoryPack;
    let pack = Arc::new(MemoryPack::default());
    let keys: Vec<String> = (0..=MAX_OVERSIZED)
        .map(|index| format!("textures/icon{index:03}"))
        .collect();
    for key in &keys {
        pack.write(&format!("{key}.png"), png(1).to_vec());
    }
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_image_work(ImageWork::inline())
        .with_fallbacks(Some(pack), None);
    atlas.require(keys.iter().map(String::as_str));
    let offered = atlas.oversized();
    assert_eq!(offered.len(), MAX_OVERSIZED);
    assert!(
        offered
            .iter()
            .all(|entry| keys[..MAX_OVERSIZED].contains(&entry.0)),
        "an entry this frame draws is never replaced by a later one"
    );
}

/// After scrolling, the icons now on screen lead the art-page order, so a bounded art page
/// drops the ones that scrolled away rather than the last keys alphabetically.
#[test]
fn priority_puts_the_textures_drawn_last_first() {
    use crate::atlas::tests::MemoryPack;
    const ICONS: usize = 18;
    const VISIBLE: usize = 15;
    let pack = Arc::new(MemoryPack::default());
    let keys: Vec<String> = (0..ICONS)
        .map(|index| format!("textures/icon{index:03}"))
        .collect();
    for key in &keys {
        pack.write(&format!("{key}.png"), png(1).to_vec());
    }
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_image_work(ImageWork::inline())
        .with_fallbacks(Some(pack), None);
    atlas.require(keys[..VISIBLE].iter().map(String::as_str));
    let scrolled = &keys[ICONS - VISIBLE..];
    atlas.require(scrolled.iter().map(String::as_str));
    let offered = atlas.oversized();
    let leading: Vec<&str> = offered
        .by_priority()
        .take(VISIBLE)
        .map(|entry| entry.0.as_str())
        .collect();
    let mut expected: Vec<&str> = scrolled.iter().map(String::as_str).collect();
    let mut sorted = leading.clone();
    sorted.sort_unstable();
    expected.sort_unstable();
    assert_eq!(sorted, expected);
    assert_eq!(offered.by_priority().count(), ICONS);
}
