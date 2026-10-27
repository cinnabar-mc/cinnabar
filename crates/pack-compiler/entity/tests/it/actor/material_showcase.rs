//! The local server's material showcase pack, compiled from its exact files.
use sha2::{Digest, Sha256};

/// One showcase file, by its pack path, read from the fixture the local server offers.
macro_rules! showcase {
    ($path:literal) => {
        (
            $path,
            include_bytes!(concat!(
                "../../../../../../tools/localserver/fixtures/material_showcase/",
                $path
            ))
            .as_slice(),
        )
    };
}

/// The showcase images, generated as the local server generates them; each one's pixels must
/// match the digest the server's own test pins.
fn showcase_images() -> Vec<(Box<str>, Vec<u8>)> {
    let pinned: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(include_bytes!(
            "../../../../../../tools/localserver/fixtures/material_showcase.pixels.json"
        ))
        .unwrap();
    let generate = |pixel: fn(u32, u32) -> [u8; 4]| {
        image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba(pixel(x, y)))
    };
    let images = [
        (
            "textures/entity/material_showcase.png",
            generate(|x, y| [40 + x as u8, 60 + y as u8, 140, 255]),
        ),
        (
            "textures/entity/material_showcase_glow.png",
            generate(|x, y| {
                if (x / 4 + y / 4) % 4 == 0 {
                    [255, 170, 40, 255]
                } else {
                    [0; 4]
                }
            }),
        ),
    ];
    assert_eq!(pinned.len(), images.len());
    images
        .into_iter()
        .map(|(path, image)| {
            let digest: String = Sha256::digest(image.as_raw())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(
                pinned[path], digest,
                "{path} matches the local server's image"
            );
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png).unwrap();
            (path.into(), png.into_inner())
        })
        .collect()
}

/// Every showcase file the client compiles; the manifest carries no entity content.
fn showcase_files() -> Vec<(Box<str>, Vec<u8>)> {
    [
        showcase!("cinnabar/material-bindings.json"),
        showcase!("entity/material_additive.entity.json"),
        showcase!("entity/material_glow.entity.json"),
        showcase!("materials/entity.material"),
        showcase!("models/entity/actor_burst.geo.json"),
        showcase!("render_controllers/material_showcase.json"),
    ]
    .into_iter()
    .map(|(path, bytes)| (path.into(), bytes.to_vec()))
    .chain(showcase_images())
    .collect()
}

#[test]
fn a_bound_glow_mask_is_uploaded_beside_the_body_it_lights() {
    let compiled = pack_entity_compiler::compile_actor_pack(showcase_files())
        .unwrap()
        .expect("the showcase pack has actors");
    let entities = &compiled.entities;
    let masks = entities
        .render
        .layers
        .iter()
        .find_map(|layer| layer.material_masks)
        .map(|index| entities.render.material_masks[index as usize])
        .expect("the bound entity's layer carries its masks");
    assert_eq!(masks.strength, 1.5);
    let glow = masks.glow.expect("the glow mask resolves to its source");
    assert_eq!(
        entities.sources[glow as usize].path.as_ref(),
        "textures/entity/material_showcase_glow.png"
    );
    let texture = compiled
        .textures
        .iter()
        .find(|texture| texture.source == glow)
        .expect("the glow mask's raster is uploaded with the pack's artwork");
    // The transparent texels between the bands stay transparent, so only the bands glow.
    let alpha = texture
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3]);
    assert!(alpha.clone().any(|alpha| alpha == 255));
    assert!(alpha.clone().any(|alpha| alpha == 0));
    let body = compiled
        .textures
        .iter()
        .find(|texture| {
            entities.sources[texture.source as usize].path.as_ref()
                == "textures/entity/material_showcase.png"
        })
        .expect("the body texture is uploaded");
    let red = body.rgba8.as_chunks::<4>().0.iter().map(|pixel| pixel[0]);
    assert_eq!(
        (red.clone().min(), red.max()),
        (Some(40), Some(103)),
        "the body is the fixture's gradient"
    );
}
