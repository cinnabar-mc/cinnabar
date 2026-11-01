//! OptiFine/MCPatcher custom skies and the Bedrock overworld cube map built from them.

use crate::{meta::ASSETS, texture::crop};
use image::RgbaImage;

/// Bedrock cube-map face written for each 3x2 cell of an OptiFine sky, in reading order.
const CUBEMAP_FACES: [u32; 6] = [5, 4, 2, 3, 0, 1];

/// The shallowest folder named `world0` (OptiFine's overworld sky folder), ending in `/`.
pub(crate) fn sky_folder<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<String> {
    paths
        .into_iter()
        .filter_map(|path| {
            let end = path
                .find("world0/")
                .filter(|start| *start == 0 || path.as_bytes()[start - 1] == b'/')?;
            Some(&path[..end + "world0/".len()])
        })
        .min_by_key(|folder| (folder.matches('/').count(), *folder))
        .map(str::to_owned)
}

/// Reads the `source` value of an OptiFine sky `.properties` file, if it names one.
pub(crate) fn sky_source(properties: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(properties);
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#') && !line.starts_with('!'))
        .filter_map(|line| {
            let split = line.find(['=', ':'])?;
            Some((line[..split].trim(), line[split + 1..].trim()))
        })
        .rfind(|(key, _)| *key == "source")
        .map(|(_, value)| value.to_owned())
        .filter(|value| !value.is_empty())
}

/// Resolves a sky `source` relative to the sky folder `folder` (ending in `/`), accepting
/// `./x.png`, `x.png`, `optifine/sky/world0/x.png` and namespaced `minecraft:optifine/...`.
pub(crate) fn source_path(folder: &str, source: &str) -> String {
    let source = source.strip_prefix("minecraft:").unwrap_or(source);
    if let Some(relative) = source.strip_prefix("./") {
        return format!("{folder}{relative}");
    }
    if source.contains('/') {
        return match folder.find(ASSETS) {
            Some(start) => format!("{}{source}", &folder[..start + ASSETS.len()]),
            None => source.to_owned(),
        };
    }
    format!("{folder}{source}")
}

/// Splits a 3x2 OptiFine sky into the six Bedrock `cubemap_<n>.png` faces, indexed by `n`.
pub(crate) fn cubemap_faces(sky: &RgbaImage) -> Option<[RgbaImage; 6]> {
    let (width, height) = (sky.width() / 3, sky.height() / 2);
    if width == 0 || height == 0 {
        return None;
    }
    let mut faces: [RgbaImage; 6] = Default::default();
    for (cell, face) in CUBEMAP_FACES.into_iter().enumerate() {
        let cell = cell as u32;
        faces[face as usize] = crop(sky, (cell % 3) * width, (cell / 3) * height, width, height);
    }
    Some(faces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_resolve_like_optifine() {
        let folder = "assets/minecraft/optifine/sky/world0/";
        assert_eq!(
            source_path(folder, "./sky1.png"),
            format!("{folder}sky1.png")
        );
        assert_eq!(source_path(folder, "sky1.png"), format!("{folder}sky1.png"));
        assert_eq!(
            source_path(folder, "minecraft:optifine/sky/world0/a.png"),
            "assets/minecraft/optifine/sky/world0/a.png"
        );
    }

    #[test]
    fn properties_name_their_source() {
        assert_eq!(
            sky_source(b"# sky\nfadeIn=1:00\nsource = ./sky1.png\n").as_deref(),
            Some("./sky1.png")
        );
        assert_eq!(sky_source(b"rotate=true\n"), None);
    }

    #[test]
    fn the_shallowest_world0_folder_wins() {
        let paths = [
            "assets/minecraft/optifine/sky/world0/a.properties",
            "assets/minecraft/mcpatcher/x/sky/world0/b.properties",
            "notworld0/c.png",
        ];
        assert_eq!(
            sky_folder(paths).as_deref(),
            Some("assets/minecraft/optifine/sky/world0/")
        );
    }

    #[test]
    fn faces_follow_the_bedrock_order() {
        let sky = RgbaImage::from_fn(6, 4, |x, y| {
            image::Rgba([(x / 2 + 3 * (y / 2)) as u8, 0, 0, 255])
        });
        let faces = cubemap_faces(&sky).unwrap();
        for (cell, face) in CUBEMAP_FACES.into_iter().enumerate() {
            assert_eq!(faces[face as usize].get_pixel(0, 0).0[0], cell as u8);
        }
    }
}
