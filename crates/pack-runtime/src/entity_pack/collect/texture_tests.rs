//! Bounded, shared colour validation and cancellation for entity texture sets.

use super::*;
use serde_json::json;

const STEMS: [&str; 2] = ["textures/0_first", "textures/1_second"];

/// Encodes solid original pixels in the requested source format.
fn solid_image(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(width, height, image::Rgb([10, 20, 30]))
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

/// Admits entity texture aliases sharing one colour image and a plain fallback.
fn texture_sets(stems: &[&str], colour: &[u8], extension: &str) -> (LayeredPackView, Vec<u8>) {
    let plain = solid_image(2, 2, image::ImageFormat::Png);
    let textures = stems
        .iter()
        .enumerate()
        .map(|(index, stem)| (index.to_string(), Value::from(*stem)))
        .collect::<Map<_, _>>();
    let mut files = vec![
        (
            "entity/moth.json".to_owned(),
            json!({"format_version": "1.10.0", "minecraft:client_entity": {"description": {
                "identifier": "example:moth", "textures": textures
            }}})
            .to_string()
            .into_bytes(),
        ),
        (format!("textures/shared.{extension}"), colour.to_vec()),
    ];
    for stem in stems {
        files.push((
            format!("{stem}.texture_set.json"),
            json!({"format_version": "1.16.100", "minecraft:texture_set": {"color": "shared"}})
                .to_string()
                .into_bytes(),
        ));
        files.push((format!("{stem}.png"), plain.clone()));
    }
    let borrowed = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    (
        LayeredPackView::new(crate::test_support::stored_stack(&borrowed)),
        plain,
    )
}

#[test]
fn oversized_entity_texture_set_colours_fall_back_before_pixel_allocation() {
    let side = u32::from(assets::MAX_ACTOR_TEXTURE_SIDE);
    for (width, height) in [(side + 1, 128), (128, side + 1)] {
        for (format, extension) in [
            (image::ImageFormat::Png, "png"),
            (image::ImageFormat::Tga, "tga"),
        ] {
            let colour = solid_image(width, height, format);
            let (view, plain) = texture_sets(&STEMS, &colour, extension);
            let (files, allocated) = test_alloc::measure(|| collect_files(&view, None, None));
            for stem in STEMS {
                let path = format!("{stem}.png");
                let collected = files
                    .iter()
                    .find(|(file, _)| file.as_ref() == path)
                    .unwrap();
                assert_eq!(collected.1, plain, "{width}x{height} {extension}");
            }
            if format == image::ImageFormat::Png {
                let pixels = width as usize * height as usize * 3;
                assert!(
                    allocated.bytes < pixels / 2,
                    "rejecting {pixels} pixel bytes allocated {} bytes",
                    allocated.bytes
                );
            }
        }
    }
}

#[test]
fn entity_texture_set_colours_keep_the_actor_dimension_boundary() {
    let colour = solid_image(
        assets::MAX_ACTOR_TEXTURE_SIDE.into(),
        1,
        image::ImageFormat::Png,
    );
    let (view, _) = texture_sets(&STEMS, &colour, "png");
    let files = collect_files(&view, None, None);
    for stem in STEMS {
        let path = format!("{stem}.png");
        let collected = files
            .iter()
            .find(|(file, _)| file.as_ref() == path)
            .unwrap();
        assert_eq!(collected.1, colour);
    }
}

#[test]
fn shared_entity_texture_set_colours_reuse_validation_conversions_and_failures() {
    const SIDE: u32 = 512;
    let stems = (0..16)
        .map(|index| format!("textures/alias_{index}"))
        .collect::<Vec<_>>();
    let stems = stems.iter().map(String::as_str).collect::<Vec<_>>();
    for (format, extension, corrupt) in [
        (image::ImageFormat::Png, "png", false),
        (image::ImageFormat::Tga, "tga", false),
        (image::ImageFormat::Jpeg, "jpg", false),
        (image::ImageFormat::Png, "png", true),
    ] {
        let mut colour = solid_image(SIDE, SIDE, format);
        if corrupt {
            colour.truncate(colour.len() / 2);
        }
        let (view, plain) = texture_sets(&stems, &colour, extension);
        let (files, allocated) = test_alloc::measure(|| collect_files(&view, None, None));
        let output_format = if format == image::ImageFormat::Tga {
            "tga"
        } else {
            "png"
        };
        let rasters = stems
            .iter()
            .map(|stem| {
                let path = format!("{stem}.{output_format}");
                &files
                    .iter()
                    .find(|(file, _)| file.as_ref() == path)
                    .unwrap()
                    .1
            })
            .collect::<Vec<_>>();
        let first = rasters[0];
        assert!(rasters.iter().all(|bytes| *bytes == first));
        if corrupt {
            assert_eq!(first, &plain);
        } else {
            let decoded = image::ImageReader::with_format(
                std::io::Cursor::new(first),
                if format == image::ImageFormat::Tga {
                    image::ImageFormat::Tga
                } else {
                    image::ImageFormat::Png
                },
            )
            .decode()
            .unwrap();
            assert_eq!((decoded.width(), decoded.height()), (SIDE, SIDE));
        }
        assert!(
            allocated.bytes < (SIDE * SIDE * 4 * 8) as usize,
            "collecting shared {extension} (corrupt={corrupt}) allocated {} bytes",
            allocated.bytes
        );
    }
}

#[test]
fn cancelled_entity_compiles_stop_collecting_texture_set_colours() {
    let colour = solid_image(2, 2, image::ImageFormat::Png);
    let (view, _) = texture_sets(&STEMS, &colour, "png");
    let view = LayeredPackView::tracked(view.shared_stack());
    let reads = || view.dependencies().unwrap().snapshot();
    assert!(
        super::super::compile_encoded(&view, None, None, None, true, false, &|| true).is_none()
    );
    assert!(
        reads().is_empty(),
        "an already cancelled join reads nothing"
    );

    let cancelled = || {
        reads().iter().any(|dependency| {
            matches!(dependency, resource_pack::PackDependency::File { path, .. }
                if path == "textures/shared.png")
        })
    };
    assert!(
        super::super::compile_encoded(&view, None, None, None, true, false, &cancelled).is_none()
    );
    assert!(cancelled(), "the first colour image was collected");
    let second = format!("{}.texture_set.json", STEMS[1]);
    assert!(
        !reads().iter().any(|dependency| {
            matches!(dependency, resource_pack::PackDependency::File { path, .. } if path == &second)
        }),
        "cancellation stops collection before the next texture set"
    );
}
