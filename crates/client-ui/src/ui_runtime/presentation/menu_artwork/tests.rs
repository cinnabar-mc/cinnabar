use super::*;
use ui_engine::textures::TITLE_KEY;

#[test]
fn server_details_art_fits_alongside_all_catalog_logos_without_home_promotions() {
    use launcher::menu::{MenuGameCard, MenuScreen, MenuServerCard, ServerDetails};
    for activities in [4, 12] {
        let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::Servers;
        view.featured = (0..13)
            .map(|index| MenuServerCard {
                name: index.to_string(),
                address: index.to_string(),
                caption: String::new(),
                image_path: format!("logo-{index}"),
                icon: None,
            })
            .collect();
        view.feeds.profile.avatar_path = "irrelevant-avatar".into();
        view.feeds.details.insert(
            "0".into(),
            ServerDetails {
                banner: "banner".into(),
                games: (0..activities)
                    .map(|index| MenuGameCard {
                        image_path: format!("activity-{index}"),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        );
        let set = ArtworkSet {
            paths: view_paths(&view),
            ..Default::default()
        };
        let mut cache = DecodeCache::default();
        for source in sources(&set) {
            let key = source.key();
            let width = key.1;
            let height = if key.0 == "banner" {
                width * 3 / 10
            } else {
                width
            };
            cache.insert(
                key,
                Artwork {
                    width,
                    height,
                    pixels: vec![255; width as usize * height as usize * 4],
                },
            );
        }
        let atlas = pack(&set, &cache, 0, true);
        assert_eq!(
            atlas.refs["banner"].uv[2] - atlas.refs["banner"].uv[0],
            SERVER_BANNER_SIDE as u16
        );
        for index in 0..13 {
            assert!(atlas.refs.contains_key(&format!("logo-{index}")));
        }
        for index in 0..activities {
            assert!(
                atlas.refs.contains_key(&format!("activity-{index}")),
                "activity {index} of {activities} missing"
            );
        }
        assert!(!atlas.refs.contains_key("irrelevant-avatar"));
        assert!(atlas.pages.len() <= render_model::MAX_UI_ART_PAGES);
    }
}

#[test]
fn settings_atlas_prepares_the_actual_profile_gamerpic_without_a_head_substitution() {
    let mut view = launcher::menu::MenuView::new(true, "Player".into());
    view.screen = launcher::menu::MenuScreen::Settings;
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.picture_path = "profile-gamerpic.png".into();
    view.feeds.home.persona_head = "persona-head.png".into();
    let set = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    let source = sources(&set)
        .into_iter()
        .find(|source| source.key().0 == view.feeds.profile.picture_path)
        .unwrap();
    let mut cache = DecodeCache::default();
    cache.insert(
        source.key(),
        Artwork {
            width: 8,
            height: 8,
            pixels: vec![255; 8 * 8 * 4],
        },
    );
    let prepared = pack(&set, &cache, 0, true);
    let picture = prepared.refs.get(&view.feeds.profile.picture_path).unwrap();
    assert_eq!(picture.uv[2] - picture.uv[0], 8);
    assert_eq!(picture.uv[3] - picture.uv[1], 8);
    assert!(!prepared.refs.contains_key(&view.feeds.home.persona_head));
}

#[test]
fn large_artwork_does_not_crowd_out_a_prioritized_gamerpic() {
    let mut set = ArtworkSet {
        paths: vec![("gamerpic".into(), THUMBNAIL_SIDE)],
        ..Default::default()
    };
    set.paths
        .extend((0..8).map(|index| (format!("large-{index}"), MAX_ARTWORK_SIDE)));
    let mut cache = DecodeCache::default();
    for source in sources(&set) {
        let side = if source.key().0 == "gamerpic" {
            THUMBNAIL_SIDE
        } else {
            MAX_ARTWORK_SIDE
        };
        cache.insert(
            source.key(),
            Artwork {
                width: side,
                height: side,
                pixels: vec![90; side as usize * side as usize * 4],
            },
        );
    }
    let packed = pack(&set, &cache, 0, true);
    assert!(
        packed.refs.contains_key("gamerpic"),
        "packed atlas must retain a prioritized gamerpic under capacity pressure"
    );
}

#[test]
fn a_full_server_catalog_does_not_crowd_out_account_pictures() {
    let mut view = launcher::menu::MenuView::new(true, "Player".into());
    view.feeds.account_active_id = Some("player".into());
    view.feeds.accounts = vec![launcher::accounts::AccountProfile {
        id: "player".into(),
        gamertag: "Player".into(),
        picture_path: Some("gamerpic.png".into()),
    }];
    view.featured = (0..MAX_ARTWORKS)
        .map(|index| launcher::menu::MenuServerCard {
            name: index.to_string(),
            address: index.to_string(),
            caption: String::new(),
            image_path: format!("server-{index}.png"),
            icon: None,
        })
        .collect();
    let set = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    assert!(
        sources(&set)
            .iter()
            .any(|source| source.key().0 == "gamerpic.png"),
        "bounded atlas must retain the visible account picture"
    );
}

#[test]
fn account_pictures_are_queued_when_the_picker_opens() {
    let mut view = launcher::menu::MenuView::new(true, "First".into());
    view.feeds.account_active_id = Some("first".into());
    view.feeds.accounts = ["first", "second"]
        .map(|id| launcher::accounts::AccountProfile {
            id: id.into(),
            gamertag: id.into(),
            picture_path: Some(format!("{id}.png")),
        })
        .into();
    let paths = view_paths(&view);
    assert!(paths.iter().any(|(path, _)| path == "first.png"));
    assert!(!paths.iter().any(|(path, _)| path == "second.png"));
    view.dialog = Some(launcher::menu::MenuDialog::Accounts);
    assert!(
        view_paths(&view)
            .iter()
            .any(|(path, _)| path == "second.png")
    );
}

#[test]
fn profile_replacement_atlas_preserves_the_portrait_fallback() {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-profile-portrait-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, png(8, 8, [40, 80, 120, 255])).unwrap();
    let portrait = path.to_string_lossy().into_owned();
    let mut view = launcher::menu::MenuView::new(true, "Fixture Player".into());
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.loaded = true;
    view.feeds.profile.avatar_loaded = true;
    view.feeds.profile.featured_screenshot_loaded = true;
    view.feeds.home.persona_head = portrait.clone();
    let home = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    let missing = cache.missing(&home);
    cache.decode(&missing);
    assert!(pack(&home, &cache, 0, true).refs.contains_key(&portrait));
    view.screen = launcher::menu::MenuScreen::Profile;
    for tab in [
        launcher::menu::ProfileTab::Overview,
        launcher::menu::ProfileTab::Stats,
    ] {
        view.profile_tab = tab;
        // A missing gamerpic and a failed gamerpic decode both use the head.
        for gamerpic in [String::new(), format!("{portrait}.missing")] {
            view.feeds.profile.picture_path = gamerpic;
            let profile = ArtworkSet {
                paths: view_paths(&view),
                ..Default::default()
            };
            let missing = cache.missing(&profile);
            cache.decode(&missing);
            let replacement = pack(&profile, &cache, 1, true);
            assert!(
                replacement.refs.contains_key(&portrait),
                "Profile {tab:?} replaced the atlas without its portrait fallback"
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

/// Encodes a solid test image without any external assets.
fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba(pixel))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// Every decode batch can be superseded before an atlas finishes packing.
#[test]
fn superseded_artwork_batches_keep_the_decode_cache_bounded() {
    let mut cache = DecodeCache::default();
    for revision in 0..2000_u32 {
        let color = [revision as u8, (revision >> 8) as u8, 20, 255];
        let set = ArtworkSet {
            oversized: [("changing".into(), png(8, 8, color).into())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let missing = cache.missing(&set);
        cache.decode(&missing);
        assert_eq!(cache.decoded.len(), 1);
        let source = sources(&set).pop().unwrap();
        assert_eq!(&cache.get(&source.key()).unwrap().pixels[..4], &color);
    }
}

/// Churning distinct paths exercises eviction even when every batch is cancelled.
#[test]
fn cancelled_artwork_batches_evict_obsolete_paths() {
    let mut cache = DecodeCache::default();
    let bytes: Arc<[u8]> = png(512, 512, [10, 20, 30, 255]).into();
    let per_image = 512 * 512 * 4;
    for revision in 0..RETAINED_DECODE_BYTES / per_image * 2 / FIRST_BATCH {
        let set = ArtworkSet {
            images: (0..FIRST_BATCH * 2)
                .map(|index| (format!("changing-{revision}-{index}"), Arc::clone(&bytes)))
                .collect(),
            ..Default::default()
        };
        let missing = cache.missing(&set);
        cache.decode(&missing[..FIRST_BATCH]);
        assert!(cache.retained_bytes() <= RETAINED_DECODE_BYTES);
        assert_eq!(
            cache.missing(&set).len(),
            FIRST_BATCH,
            "the decoded art stays"
        );
    }
}

/// Art a fixture set draws, each image a quarter of the retention budget.
fn quarter_budget_set(cache: &mut DecodeCache, names: &[&str]) -> ArtworkSet {
    let set = ArtworkSet {
        paths: names.iter().map(|name| ((*name).into(), 64)).collect(),
        ..Default::default()
    };
    let pixels = RETAINED_DECODE_BYTES / 4;
    for source in sources(&set) {
        cache.insert(
            source.key(),
            Artwork {
                width: (pixels / 4) as u32,
                height: 1,
                pixels: vec![0; pixels],
            },
        );
    }
    set
}

/// Over budget, the art drawn longest ago goes first and evicted art decodes again on demand.
#[test]
fn decode_cache_evicts_least_recently_drawn_art_by_bytes() {
    let mut cache = DecodeCache::default();
    let oldest = quarter_budget_set(&mut cache, &["oldest"]);
    assert!(cache.missing(&oldest).is_empty());
    let older = quarter_budget_set(&mut cache, &["older-a", "older-b"]);
    assert!(cache.missing(&older).is_empty());
    assert_eq!(cache.decoded.len(), 3, "under budget nothing is evicted");
    let current = quarter_budget_set(&mut cache, &["current"]);
    assert!(cache.missing(&current).is_empty());
    assert!(cache.retained_bytes() <= RETAINED_DECODE_BYTES);
    assert!(cache.get(&sources(&current)[0].key()).is_some());
    for source in sources(&older) {
        assert!(cache.get(&source.key()).is_some(), "newer art was evicted");
    }
    assert_eq!(cache.missing(&oldest).len(), 1, "evicted art decodes again");
}

/// Art the current set draws is never evicted, even when it alone exceeds the budget.
#[test]
fn drawn_art_outlives_the_byte_budget() {
    let mut cache = DecodeCache::default();
    let names = ["a", "b", "c", "d", "e", "f"];
    let set = quarter_budget_set(&mut cache, &names);
    assert!(cache.missing(&set).is_empty());
    assert!(cache.retained_bytes() > RETAINED_DECODE_BYTES);
    assert!(cache.missing(&set).is_empty(), "drawn art was evicted");
    cache.missing(&ArtworkSet::default());
    assert!(cache.retained_bytes() <= RETAINED_DECODE_BYTES);
}

#[test]
fn decode_cache_charges_spare_capacity_and_releases_replaced_pixels() {
    let mut path = String::with_capacity(256);
    path.push_str("spare-capacity");
    let key = (path, THUMBNAIL_SIDE, None);
    let mut pixels = Vec::with_capacity(4096);
    pixels.extend_from_slice(&[10, 20, 30, 255]);
    let expected = size_of::<(DecodeKey, Cached)>()
        + 2 * size_of::<usize>()
        + size_of::<Artwork>()
        + key.0.capacity()
        + pixels.capacity();
    let replacement_key = key.clone();
    let mut cache = DecodeCache::default();
    cache.insert(
        key,
        Artwork {
            width: 1,
            height: 1,
            pixels,
        },
    );
    let cached = cache.decoded.values().next().unwrap();
    assert_eq!(cache.retained_bytes(), expected);
    let old_pixels = Arc::downgrade(&cached.art);
    cache.insert(
        replacement_key,
        Artwork {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        },
    );
    assert_eq!(cache.decoded.len(), 1);
    assert!(old_pixels.upgrade().is_none());
    assert!(cache.retained_bytes() < expected);
}

#[test]
fn drawing_cached_art_refreshes_its_eviction_priority() {
    let mut cache = DecodeCache::default();
    let reused = quarter_budget_set(&mut cache, &["reused"]);
    cache.missing(&reused);
    let older = quarter_budget_set(&mut cache, &["older"]);
    cache.missing(&older);
    cache.missing(&reused);
    let current = quarter_budget_set(&mut cache, &["current-a", "current-b"]);
    cache.missing(&current);
    assert!(cache.retained_bytes() <= RETAINED_DECODE_BYTES);
    assert!(cache.get(&sources(&reused)[0].key()).is_some());
    assert!(cache.get(&sources(&older)[0].key()).is_none());
    assert!(cache.missing(&current).is_empty());
}

#[test]
fn evicted_art_decodes_again_with_the_same_pixels() {
    let mut cache = DecodeCache::default();
    let image: Arc<[u8]> = png(8, 8, [10, 20, 30, 128]).into();
    let set = ArtworkSet {
        images: vec![("redecode".into(), image)],
        ..Default::default()
    };
    let missing = cache.missing(&set);
    cache.decode(&missing);
    let key = sources(&set)[0].key();
    let expected = cache.get(&key).unwrap().pixels.clone();
    let current = quarter_budget_set(&mut cache, &["a", "b", "c", "d"]);
    cache.missing(&current);
    assert!(cache.get(&key).is_none());
    let missing = cache.missing(&set);
    assert_eq!(missing.len(), 1);
    cache.decode(&missing);
    assert_eq!(cache.get(&key).unwrap().pixels, expected);
    assert!(cache.retained_bytes() <= RETAINED_DECODE_BYTES);
}

#[test]
fn small_art_stays_cached_when_old_large_art_is_evicted() {
    let mut cache = DecodeCache::default();
    let large = quarter_budget_set(&mut cache, &["large-a", "large-b", "large-c"]);
    cache.missing(&large);
    let image: Arc<[u8]> = png(2, 2, [255; 4]).into();
    let small = ArtworkSet {
        images: (0..FIRST_BATCH)
            .map(|index| (format!("small-{index}"), Arc::clone(&image)))
            .collect(),
        ..Default::default()
    };
    let missing = cache.missing(&small);
    cache.decode(&missing);
    let current = quarter_budget_set(&mut cache, &["current"]);
    cache.missing(&current);
    assert!(cache.retained_bytes() <= RETAINED_DECODE_BYTES);
    assert!(cache.missing(&small).is_empty());
    assert!(
        sources(&large)
            .iter()
            .any(|source| cache.get(&source.key()).is_none())
    );
}

#[test]
fn replacing_pack_bytes_invalidates_decoded_artwork() {
    let mut cache = DecodeCache::default();
    for color in [[200, 30, 40, 255], [20, 220, 30, 255]] {
        let set = ArtworkSet {
            oversized: [(TITLE_KEY.to_owned(), png(900, 300, color).into())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let missing = cache.missing(&set);
        cache.decode(&missing);
        let atlas = pack(&set, &cache, 0, true);
        let art = atlas.refs[&format!("{SERVER_ART_PREFIX}{TITLE_KEY}")];
        let page = &atlas.pages[usize::from(art.page)];
        let at = (u32::from(art.uv[1]) * page.dimensions()[0] + u32::from(art.uv[0])) as usize * 4;
        assert_eq!(&page.pixels()[at..at + 4], &color);
    }
}

#[test]
fn a_superseded_prepared_atlas_cannot_be_installed() {
    let mut loader = ArtworkLoader::default();
    assert!(loader.ready.is_some());
    loader.request(ArtworkSet::default());
    assert!(loader.ready.is_none());
    assert!(loader.take().is_none());
}

// A large server texture (Zeqa's 1992x669 title) keeps a whole art page of
// detail under its own key, not the 256px server-page downscale.
#[test]
fn oversized_server_textures_keep_full_resolution() {
    let bytes: std::sync::Arc<[u8]> = png(1992, 669, [200, 30, 40, 255]).into();
    let set = ArtworkSet {
        paths: Vec::new(),
        oversized: [(TITLE_KEY.to_owned(), bytes)].into_iter().collect(),
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    let missing = cache.missing(&set);
    cache.decode(&missing);
    let atlas = pack(&set, &cache, 0, true);
    let art = atlas.refs[&format!("{SERVER_ART_PREFIX}{TITLE_KEY}")];
    let [u0, v0, u1, v1] = art.uv;
    assert_eq!([u1 - u0, v1 - v0], [1022, 343]);
    // Cinnabar's logo keeps the plain title key.
    assert_ne!(atlas.refs[TITLE_KEY].uv, art.uv);
}

// Artwork stays straight alpha, as the UI shader samples it.
#[test]
fn artwork_is_straight_alpha() {
    let (pixels, _, _) = decode_bytes(&png(4, 4, [200, 100, 50, 128]), 64).unwrap();
    assert_eq!(&pixels[..4], &[200, 100, 50, 128]);
    let (scaled, width, _) = decode_bytes(&png(128, 128, [200, 100, 50, 128]), 64).unwrap();
    assert_eq!(width, 64);
    assert!(
        scaled[..3]
            .iter()
            .zip([200, 100, 50])
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
}
#[test]
fn grayscale_conversion_obeys_the_decode_memory_budget() {
    let mut bytes = Vec::new();
    image::GrayImage::new(4096, 4096)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    assert!(decode_bytes(&bytes, 128).is_none());
}

#[test]
fn oversized_source_sets_cannot_defeat_the_decode_cache_bound() {
    let bytes: Arc<[u8]> = png(2, 2, [255; 4]).into();
    let set = ArtworkSet {
        oversized: (0..MAX_ARTWORKS + 1)
            .map(|i| (format!("texture-{i}"), bytes.clone()))
            .collect(),
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    let missing = cache.missing(&set);
    assert!(missing.len() <= MAX_ARTWORKS);
    cache.decode(&missing);
    assert!(cache.decoded.len() <= MAX_ARTWORKS);
}

/// Unresized 8-bit art converts straight to RGBA8 with the float round trip's exact bytes.
#[test]
fn direct_rgba8_conversion_matches_the_float_round_trip() {
    use image::{DynamicImage, ImageBuffer};
    let byte = |x: u32, y: u32, salt: u32| ((x * 7 + y * 13 + salt) % 256) as u8;
    let images = [
        DynamicImage::ImageLuma8(ImageBuffer::from_fn(256, 1, |x, _| image::Luma([x as u8]))),
        DynamicImage::ImageLumaA8(ImageBuffer::from_fn(256, 256, |x, y| {
            image::LumaA([x as u8, y as u8])
        })),
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(256, 256, |x, y| {
            image::Rgb([x as u8, y as u8, byte(x, y, 1)])
        })),
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(256, 256, |x, y| {
            image::Rgba([x as u8, y as u8, byte(x, y, 2), byte(y, x, 3)])
        })),
        DynamicImage::ImageRgba16(ImageBuffer::from_fn(256, 256, |x, y| {
            image::Rgba([(x * 257) as u16, (y * 255) as u16, 32_768, 65_535])
        })),
    ];
    for image in images {
        let color = image.color();
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let expected = DynamicImage::ImageRgba32F(image.into_rgba32f()).into_rgba8();
        let (pixels, width, height) = decode_bytes(&bytes, 256).unwrap();
        assert_eq!((width, height), expected.dimensions(), "{color:?}");
        assert!(pixels == expected.into_raw(), "{color:?} pixels differ");
    }
}

#[test]
fn unresized_rgba8_reuses_decoded_storage() {
    let image = image::RgbaImage::from_pixel(32, 32, image::Rgba([10, 20, 30, 128]));
    let pixels = image.as_raw().as_ptr();
    let direct = fitted_rgba8(image::DynamicImage::ImageRgba8(image));
    assert_eq!(direct.as_raw().as_ptr(), pixels);
    assert!(direct.pixels().all(|pixel| pixel.0 == [10, 20, 30, 128]));
}

#[test]
fn unresized_high_precision_art_keeps_float_round_trip_rounding() {
    use image::{DynamicImage, ImageBuffer};
    let images = [
        DynamicImage::ImageLuma16(ImageBuffer::from_fn(256, 256, |x, y| {
            image::Luma([(x * 256 + y) as u16])
        })),
        DynamicImage::ImageLumaA16(ImageBuffer::from_fn(256, 256, |x, y| {
            image::LumaA([(x * 256 + y) as u16, (y * 256 + x) as u16])
        })),
        DynamicImage::ImageRgb16(ImageBuffer::from_fn(256, 256, |x, y| {
            image::Rgb([(x * 256 + y) as u16, 32_768, 65_535])
        })),
        DynamicImage::ImageRgb32F(ImageBuffer::from_fn(256, 1, |x, _| {
            image::Rgb([x as f32 / 255.0, -0.5, 1.5])
        })),
        DynamicImage::ImageRgba32F(ImageBuffer::from_fn(256, 1, |x, _| {
            image::Rgba([x as f32 / 255.0, 0.5, 0.0, (255 - x) as f32 / 255.0])
        })),
    ];
    for image in images {
        let expected = DynamicImage::ImageRgba32F(image.clone().into_rgba32f()).into_rgba8();
        assert_eq!(fitted_rgba8(image), expected);
    }
}

#[test]
fn repeated_artwork_paths_do_not_exclude_later_unique_sources() {
    let set = ArtworkSet {
        paths: std::iter::repeat_n(("same".into(), 128), MAX_ARTWORKS)
            .chain([("later".into(), 128)])
            .collect(),
        ..Default::default()
    };
    assert_eq!(sources(&set).len(), 2);
}

/// The worlds tab packs each world's captured view under the key its row draws.
#[test]
fn world_thumbnails_are_packed_from_memory_on_the_worlds_tab() {
    use launcher::local_worlds::world::{WorldThumbnail, thumbnail_key};
    use launcher::menu::{LocalWorldCard, MenuScreen};
    let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Play;
    view.local_worlds = vec![LocalWorldCard {
        thumbnail: Some(WorldThumbnail::new(
            "0123456789abcdef",
            Arc::from(&b"jpeg"[..]),
        )),
        ..Default::default()
    }]
    .into();
    let set = ArtworkSet {
        images: view_images(&view, &[]),
        ..Default::default()
    };
    let keys: Vec<_> = sources(&set).iter().map(|source| source.key().0).collect();
    assert_eq!(keys, [thumbnail_key("0123456789abcdef")]);

    view.screen = MenuScreen::Home;
    assert!(
        view_images(&view, &[]).is_empty(),
        "other screens leave atlas room to theirs"
    );
}

/// Mods-tab icons pack from their validated source under the key each card draws.
#[test]
fn mod_icons_are_packed_from_memory_on_the_mods_tab() {
    use launcher::menu::{MenuScreen, settings_sections::SettingSection};
    use launcher::mods::{InstalledMod, Library, Snapshot};
    let icon = |id| InstalledMod {
        id,
        file: format!("{id}.wasm"),
        bytes: 1,
        permissions: Vec::new(),
        metadata: ui::mod_metadata::ModMetadata {
            icon: (id == 1).then(|| Arc::from(&b"png"[..])),
            ..Default::default()
        },
    };
    let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Settings;
    view.settings.section = SettingSection::Mods.definition().index;
    view.mods = Arc::new(Snapshot {
        library: Library {
            installed: vec![icon(1), icon(2)],
            ..Default::default()
        },
        ..Default::default()
    });
    let keys: Vec<_> = view_images(&view, &[])
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert_eq!(keys, [icon(1).icon_key().unwrap()]);
    view.settings.section = SettingSection::Video.definition().index;
    assert!(view_images(&view, &[]).is_empty());
}
