//! Reads a Java pack's relevant files and assembles the translated Bedrock pack.

use crate::{
    Layout, TranslateError, Translation, hud, items, manifest,
    meta::{TEXTURES, detect_layout, read_meta},
    names, sky,
    texture::{Budget, crop, decode, dimensions, encode, strip_frame},
    tga::{self, TgaKind},
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
    /// Pixels still to be encoded as TGA.
    Tga(RgbaImage),
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

/// PNG paths below `folder`, each paired with a key derived from its path. With `nested` false
/// only the folder's direct children count: vanilla block, item and armour folders are flat, and
/// textures in subfolders belong to custom models that must not replace vanilla artwork.
fn pngs_below<'a>(
    paths: &'a [String],
    folder: &str,
    nested: bool,
    key: impl Fn(&'a str) -> String,
) -> Vec<(String, &'a str)> {
    paths
        .iter()
        .filter(|path| {
            path.strip_prefix(folder).is_some_and(|relative| {
                relative.ends_with(".png") && (nested || !relative.contains('/'))
            })
        })
        .map(|path| (key(path.as_str()), path.as_str()))
        .collect()
}

/// The top square frame of a vertical animation strip, or the image itself.
fn first_frame(image: RgbaImage, budget: &Budget) -> Option<RgbaImage> {
    match strip_frame(image.width(), image.height()) {
        Some(side) => crop(&image, 0, 0, side, side, budget),
        None => Some(image),
    }
}

/// Translates a block texture: Bedrock-animated textures keep their strip, others their first
/// frame. Returns `None` for invalid PNGs.
fn block(layout: Layout, name: &str, bytes: Vec<u8>, budget: &Budget) -> Option<(String, Output)> {
    let bedrock = names::block_path(layout, name);
    let (width, height) = dimensions(&bytes)?;
    let output = match strip_frame(width, height).filter(|_| !names::is_flipbook(&bedrock)) {
        Some(side) => Output::Image(crop(&decode(&bytes, budget)?, 0, 0, side, side, budget)?),
        None => Output::Raw(bytes),
    };
    Some((format!("textures/{bedrock}.png"), output))
}

/// A translated item texture and, when potions or atlases compose it, its full decoded image.
type ItemResult = Option<((String, Output), Option<(String, RgbaImage)>)>;

/// Translates an item texture. Source alpha is kept: the client applies Bedrock's own alpha
/// handling to translated packs as to any other pack. Pixels are decoded only when the texture
/// is cropped or composed into potions or atlases.
fn item(layout: Layout, name: &str, bytes: Vec<u8>, budget: &Budget) -> ItemResult {
    let (width, height) = dimensions(&bytes)?;
    if let Some(atlas) = items::legacy_strip_atlas(layout, name, width, height) {
        return Some((
            (format!("textures/items/{atlas}.png"), Output::Raw(bytes)),
            None,
        ));
    }
    let bedrock = names::item_path(layout, name);
    let frame = strip_frame(width, height).filter(|_| !names::is_flipbook(&bedrock));
    let composes = items::composes(name);
    let image = if frame.is_some() || composes {
        Some(decode(&bytes, budget)?)
    } else {
        None
    };
    let output = match (frame, &image) {
        (Some(side), Some(image)) => Output::Image(crop(image, 0, 0, side, side, budget)?),
        _ => Output::Raw(bytes),
    };
    let composed = image
        .filter(|_| composes)
        .map(|image| (name.to_owned(), image));
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
    budget: &Budget,
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
            return source
                .load(&image_path)
                .and_then(|bytes| decode(&bytes, budget));
        }
    }
    None
}

/// Decodes HUD textures in parallel, dropping invalid PNGs.
fn decode_all(files: Vec<(String, Vec<u8>)>, budget: &Budget) -> BTreeMap<String, RgbaImage> {
    files
        .into_par_iter()
        .filter_map(|(key, bytes)| Some((key, decode(&bytes, budget)?)))
        .collect()
}

/// Rewrites PNG outputs whose vanilla texture is a TGA, which would otherwise be shadowed: cutouts
/// become TGA files, and tint and dye masks are dropped because their alpha means something else.
fn retarget_tga(out: &mut BTreeMap<String, Output>, budget: &Budget) {
    let targets: Vec<(String, TgaKind)> = out
        .keys()
        .filter_map(|path| {
            let target = path.strip_prefix("textures/")?.strip_suffix(".png")?;
            Some((path.clone(), tga::kind(target)?))
        })
        .collect();
    for (path, kind) in targets {
        let Some(output) = out.remove(&path) else {
            continue;
        };
        if kind != TgaKind::Cutout {
            continue;
        }
        let image = match output {
            Output::Raw(bytes) => decode(&bytes, budget),
            Output::Image(image) | Output::Tga(image) => Some(image),
        };
        if let Some(image) = image {
            let target = &path[..path.len() - ".png".len()];
            out.insert(format!("{target}.tga"), Output::Tga(image));
        }
    }
}

/// Composes Bedrock's tint-mask grass sides from the Java base and overlay block textures named
/// for `layout`, when the pack overrides all of a mask's sources.
fn tint_mask_outputs(
    layout: Layout,
    blocks: &[(String, Vec<u8>)],
    budget: &Budget,
) -> Vec<(String, Output)> {
    let folder = layout.folders().0;
    let find = |key: &str| {
        let name = key
            .strip_prefix(folder.trim_end_matches('/'))?
            .strip_prefix('/')?;
        let (_, bytes) = blocks.iter().find(|(stem, _)| stem == name)?;
        first_frame(decode(bytes, budget)?, budget)
    };
    tga::tint_masks(layout)
        .filter_map(|(target, sources)| {
            let base = find(sources.base)?;
            let overlay = match sources.overlay {
                Some(key) => Some(find(key)?),
                None => None,
            };
            let mask = tga::tint_mask(&base, overlay.as_ref(), budget)?;
            Some((format!("textures/{target}.tga"), Output::Tga(mask)))
        })
        .collect()
}

/// See [`crate::translate`].
pub(crate) fn translate(
    paths: &[String],
    read: impl FnMut(&str) -> Option<Vec<u8>>,
    name: &str,
) -> Result<Translation, TranslateError> {
    translate_within(paths, read, name, &Budget::new())
}

/// Translates within an explicit pixel `budget`.
pub(crate) fn translate_within(
    paths: &[String],
    read: impl FnMut(&str) -> Option<Vec<u8>>,
    name: &str,
    budget: &Budget,
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
    let stem_key = |path: &str| stem(path).to_owned();
    let blocks = source.load_all(pngs_below(
        &paths,
        &format!("{TEXTURES}{block_folder}"),
        false,
        stem_key,
    ));
    let item_files = source.load_all(pngs_below(
        &paths,
        &format!("{TEXTURES}{item_folder}"),
        false,
        stem_key,
    ));
    let mut armor_paths = pngs_below(&paths, &format!("{TEXTURES}models/armor/"), false, |path| {
        format!("{}.png", stem(path))
    });
    for (folder, layer) in [("humanoid", "1"), ("humanoid_leggings", "2")] {
        let folder = format!("{TEXTURES}entity/equipment/{folder}/");
        armor_paths.extend(pngs_below(&paths, &folder, false, |path| {
            equipment_name(stem(path), layer)
        }));
    }
    let armors = source.load_all(armor_paths);
    let sprite_folder = format!("{TEXTURES}gui/sprites/hud/");
    let sprites = source.load_all(pngs_below(&paths, &sprite_folder, true, |path| {
        path[sprite_folder.len()..path.len() - ".png".len()].to_owned()
    }));
    let sky_image = read_sky(&paths, &mut source, budget);

    let masks = tint_mask_outputs(layout, &blocks, budget);
    let sources = blocks.len() + item_files.len();
    let block_outputs: Vec<_> = blocks
        .into_par_iter()
        .filter_map(|(name, bytes)| block(layout, &name, bytes, budget))
        .collect();
    let item_outputs: Vec<_> = item_files
        .into_par_iter()
        .filter_map(|(name, bytes)| item(layout, &name, bytes, budget))
        .collect();
    let invalid = sources - block_outputs.len() - item_outputs.len();
    let mut sprites = decode_all(sprites, budget);
    let icons_image = icons.as_deref().and_then(|bytes| decode(bytes, budget));
    let widgets = widgets.and_then(|bytes| decode(&bytes, budget));

    let mut out: BTreeMap<String, Output> = BTreeMap::new();
    if let Some(bytes) = pack_icon.filter(|bytes| dimensions(bytes).is_some()) {
        out.insert("pack_icon.png".into(), Output::Raw(bytes));
    }
    if let Some(faces) = sky_image
        .as_ref()
        .and_then(|sky| sky::cubemap_faces(sky, budget))
    {
        for (index, face) in faces.into_iter().enumerate() {
            out.insert(
                format!("textures/environment/overworld_cubemap/cubemap_{index}.png"),
                Output::Image(face),
            );
        }
    }
    if let Some(bytes) = icons.filter(|_| icons_image.is_some()) {
        out.insert("textures/gui/icons.png".into(), Output::Raw(bytes));
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
    for (path, image) in items::potion_textures(&composed, layout, budget)
        .into_iter()
        .chain(items::atlas_textures(&composed, budget))
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
    let mut hud_sprites = icons_image
        .as_ref()
        .map(|icons| hud::legacy_sprites(icons, budget))
        .unwrap_or_default();
    let widget_rect = |region| {
        let widgets = widgets.as_ref()?;
        hud::crop_rect(widgets, region, hud::atlas_scale(widgets), budget)
    };
    let hotbar = sprites
        .remove("hotbar")
        .or_else(|| widget_rect(hud::LEGACY_HOTBAR));
    let selection = sprites
        .remove("hotbar_selection")
        .or_else(|| widget_rect(hud::LEGACY_SELECTION));
    hud_sprites.extend(sprites);
    for (path, image) in
        hud::hud_textures(&hud_sprites, hotbar.as_ref(), selection.as_ref(), budget)
    {
        out.insert(path, Output::Image(image));
    }
    if icons_image.is_none()
        && let Some(rebuilt) = hud::icons_from_sprites(&hud_sprites, budget)
    {
        out.insert("textures/gui/icons.png".into(), Output::Image(rebuilt));
    }
    retarget_tga(&mut out, budget);
    out.extend(masks);

    if budget.exhausted() {
        return Err(TranslateError::TooLarge);
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
            Output::Tga(image) => (path, tga::encode(&image)),
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
