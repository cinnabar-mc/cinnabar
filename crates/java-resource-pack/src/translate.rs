//! Reads a Java pack's relevant files and assembles the translated Bedrock pack.

use crate::{
    Layout, TranslateError, Translation, hud, items, manifest,
    meta::{TEXTURES, detect_layout, read_meta},
    names, sky,
    texture::{crop, decode, dimensions, encode, fix_alpha, strip_frame},
};
use image::RgbaImage;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// A translated file, encoded only once every texture is known.
enum Output {
    /// Source bytes copied unchanged.
    Raw(Vec<u8>),
    /// Pixels still to be encoded as PNG.
    Image(RgbaImage),
}

/// Reads source files in path order, hashing what it reads and counting what it cannot read.
struct Source<R> {
    read: R,
    hasher: Sha256,
    skipped: usize,
}

impl<R: FnMut(&str) -> Option<Vec<u8>>> Source<R> {
    /// Reads `path`, folding its name and bytes into the content hash.
    fn load(&mut self, path: &str) -> Option<Vec<u8>> {
        let bytes = (self.read)(path);
        match &bytes {
            Some(bytes) => {
                for part in [path.as_bytes(), bytes.as_slice()] {
                    self.hasher.update((part.len() as u64).to_le_bytes());
                    self.hasher.update(part);
                }
            }
            None => self.skipped += 1,
        }
        bytes
    }

    /// Reads every path in `paths`, pairing each readable file with its key.
    fn load_all(&mut self, paths: Vec<(String, &str)>) -> Vec<(String, Vec<u8>)> {
        paths
            .into_iter()
            .filter_map(|(key, path)| Some((key, self.load(path)?)))
            .collect()
    }
}

/// The base name of `path` without its `.png` extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.strip_suffix(".png").unwrap_or(name)
}

/// PNG paths below `folder`, each paired with a key derived from its path.
fn pngs_below<'a>(
    paths: &'a [String],
    folder: &str,
    key: impl Fn(&'a str) -> String,
) -> Vec<(String, &'a str)> {
    paths
        .iter()
        .filter(|path| path.starts_with(folder) && path.ends_with(".png"))
        .map(|path| (key(path.as_str()), path.as_str()))
        .collect()
}

/// Translates a block texture: Bedrock-animated textures keep their strip, others their first
/// frame. Returns `None` for invalid PNGs.
fn block(layout: Layout, name: &str, bytes: Vec<u8>) -> Option<(String, Output)> {
    let bedrock = names::block_path(layout, name);
    let (width, height) = dimensions(&bytes)?;
    let output = match strip_frame(width, height).filter(|_| !names::is_flipbook(&bedrock)) {
        Some(side) => Output::Image(crop(&decode(&bytes)?, 0, 0, side, side)),
        None => Output::Raw(bytes),
    };
    Some((format!("textures/{bedrock}.png"), output))
}

/// A translated item texture and, when potions or atlases compose it, its full decoded image.
type ItemResult = Option<((String, Output), Option<(String, RgbaImage)>)>;

/// Translates an item texture with Bedrock's item alpha testing applied.
fn item(layout: Layout, name: &str, bytes: Vec<u8>) -> ItemResult {
    let mut image = decode(&bytes)?;
    let changed = fix_alpha(&mut image);
    let bedrock = names::item_path(layout, name);
    let frame =
        strip_frame(image.width(), image.height()).filter(|_| !names::is_flipbook(&bedrock));
    let output = match frame {
        Some(side) => Output::Image(crop(&image, 0, 0, side, side)),
        None if changed => Output::Image(image.clone()),
        None => Output::Raw(bytes),
    };
    let composed = items::composes(name).then(|| (name.to_owned(), image));
    Some(((format!("textures/{bedrock}.png"), output), composed))
}

/// Names a 1.21.2+ equipment layer like the legacy `models/armor` files:
/// `<material>_layer_<n>[_overlay].png`.
fn equipment_name(stem: &str, layer: &str) -> String {
    let (material, suffix) = stem
        .strip_suffix("_overlay")
        .map_or((stem, ""), |material| (material, "_overlay"));
    let material = if material == "turtle_scute" {
        "turtle"
    } else {
        material
    };
    format!("{material}_layer_{layer}{suffix}.png")
}

/// Reads the first OptiFine overworld sky, in path order, whose source image exists.
fn read_sky<R: FnMut(&str) -> Option<Vec<u8>>>(
    paths: &[String],
    source: &mut Source<R>,
) -> Option<RgbaImage> {
    let folder = sky::sky_folder(paths.iter().map(String::as_str))?;
    for path in paths
        .iter()
        .filter(|path| path.starts_with(&folder) && path.ends_with(".properties"))
    {
        let Some(sky_source) = source.load(path).and_then(|bytes| sky::sky_source(&bytes)) else {
            continue;
        };
        let image_path = sky::source_path(&folder, &sky_source);
        if paths.binary_search(&image_path).is_ok() {
            return source.load(&image_path).and_then(|bytes| decode(&bytes));
        }
    }
    None
}

/// Decodes HUD textures in parallel, optionally with alpha testing applied.
fn decode_all(files: Vec<(String, Vec<u8>)>, alpha_fix: bool) -> BTreeMap<String, RgbaImage> {
    files
        .into_par_iter()
        .filter_map(|(key, bytes)| {
            let mut image = decode(&bytes)?;
            if alpha_fix {
                fix_alpha(&mut image);
            }
            Some((key, image))
        })
        .collect()
}

/// See [`crate::translate`].
pub(crate) fn translate(
    paths: &[String],
    read: impl FnMut(&str) -> Option<Vec<u8>>,
    name: &str,
) -> Result<Translation, TranslateError> {
    let mut paths = paths.to_vec();
    paths.sort();
    let mut source = Source {
        read,
        hasher: Sha256::new(),
        skipped: 0,
    };
    let has = |path: &str| {
        paths
            .binary_search_by(|probe| probe.as_str().cmp(path))
            .is_ok()
    };
    let mut optional = |path: &str| has(path).then(|| source.load(path)).flatten();
    let meta = read_meta(optional("pack.mcmeta").as_deref());
    let pack_icon = optional("pack.png");
    let icons = optional(&format!("{TEXTURES}gui/icons.png"));
    let widgets = optional(&format!("{TEXTURES}gui/widgets.png"));
    let particles = optional(&format!("{TEXTURES}particle/particles.png"));

    let layout = detect_layout(paths.iter().map(String::as_str), meta.format);
    let (block_folder, item_folder) = layout.folders();
    let blocks = source.load_all(pngs_below(
        &paths,
        &format!("{TEXTURES}{block_folder}"),
        |path| stem(path).to_owned(),
    ));
    let item_files = source.load_all(pngs_below(
        &paths,
        &format!("{TEXTURES}{item_folder}"),
        |path| stem(path).to_owned(),
    ));
    let mut armor_paths = pngs_below(&paths, &format!("{TEXTURES}models/armor/"), |path| {
        format!("{}.png", stem(path))
    });
    for (folder, layer) in [("humanoid", "1"), ("humanoid_leggings", "2")] {
        let folder = format!("{TEXTURES}entity/equipment/{folder}/");
        armor_paths.extend(pngs_below(&paths, &folder, |path| {
            equipment_name(stem(path), layer)
        }));
    }
    let armors = source.load_all(armor_paths);
    let sprite_folder = format!("{TEXTURES}gui/sprites/hud/");
    let sprites = source.load_all(pngs_below(&paths, &sprite_folder, |path| {
        path[sprite_folder.len()..path.len() - ".png".len()].to_owned()
    }));
    let sky_image = read_sky(&paths, &mut source);

    let sources = blocks.len() + item_files.len();
    let block_outputs: Vec<_> = blocks
        .into_par_iter()
        .filter_map(|(name, bytes)| block(layout, &name, bytes))
        .collect();
    let item_outputs: Vec<_> = item_files
        .into_par_iter()
        .filter_map(|(name, bytes)| item(layout, &name, bytes))
        .collect();
    let invalid = sources - block_outputs.len() - item_outputs.len();
    let mut sprites = decode_all(sprites, true);
    let icons = icons.and_then(|bytes| {
        let mut image = decode(&bytes)?;
        fix_alpha(&mut image);
        Some(image)
    });
    let widgets = widgets.and_then(|bytes| decode(&bytes));

    let mut out: BTreeMap<String, Output> = BTreeMap::new();
    if let Some(bytes) = pack_icon.filter(|bytes| dimensions(bytes).is_some()) {
        out.insert("pack_icon.png".into(), Output::Raw(bytes));
    }
    if let Some(faces) = sky_image.as_ref().and_then(sky::cubemap_faces) {
        for (index, face) in faces.into_iter().enumerate() {
            out.insert(
                format!("textures/environment/overworld_cubemap/cubemap_{index}.png"),
                Output::Image(face),
            );
        }
    }
    if let Some(icons) = &icons {
        out.insert(
            "textures/gui/icons.png".into(),
            Output::Image(icons.clone()),
        );
    }
    if let Some(bytes) = particles.filter(|bytes| dimensions(bytes).is_some()) {
        out.insert("textures/particle/particles.png".into(), Output::Raw(bytes));
    }
    out.extend(block_outputs);
    let mut composed = HashMap::new();
    for (output, image) in item_outputs {
        out.insert(output.0, output.1);
        composed.extend(image);
    }
    for (path, image) in items::potion_textures(&composed, layout)
        .into_iter()
        .chain(items::atlas_textures(&composed))
    {
        out.insert(path, Output::Image(image));
    }
    for (file, bytes) in armors {
        if dimensions(&bytes).is_some() {
            let path = format!("textures/models/armor/{}", names::armor_name(&file));
            out.insert(path, Output::Raw(bytes));
        }
    }
    // Crop the legacy atlases first so 1.20.2+ sprites, when present, take precedence.
    let mut hud_sprites = icons.as_ref().map(hud::legacy_sprites).unwrap_or_default();
    let hotbar = sprites.remove("hotbar").or_else(|| {
        let widgets = widgets.as_ref()?;
        Some(hud::crop_rect(
            widgets,
            hud::LEGACY_HOTBAR,
            hud::atlas_scale(widgets),
        ))
    });
    let selection = sprites.remove("hotbar_selection").or_else(|| {
        let widgets = widgets.as_ref()?;
        Some(hud::crop_rect(
            widgets,
            hud::LEGACY_SELECTION,
            hud::atlas_scale(widgets),
        ))
    });
    hud_sprites.extend(sprites);
    for (path, image) in hud::hud_textures(&hud_sprites, hotbar.as_ref(), selection.as_ref()) {
        out.insert(path, Output::Image(image));
    }
    if icons.is_none()
        && let Some(rebuilt) = hud::icons_from_sprites(&hud_sprites)
    {
        out.insert("textures/gui/icons.png".into(), Output::Image(rebuilt));
    }

    if !out.keys().any(|path| path.starts_with("textures/")) {
        return Err(TranslateError::NothingToTranslate);
    }
    let digest: [u8; 32] = source.hasher.finalize().into();
    let mut files: Vec<(String, Vec<u8>)> = out
        .into_par_iter()
        .map(|(path, output)| match output {
            Output::Raw(bytes) => (path, bytes),
            Output::Image(image) => (path, encode(&image)),
        })
        .collect();
    files.push((
        "manifest.json".into(),
        manifest::manifest(name, &meta.description, &digest),
    ));
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(Translation {
        files,
        layout,
        skipped: source.skipped + invalid,
    })
}
