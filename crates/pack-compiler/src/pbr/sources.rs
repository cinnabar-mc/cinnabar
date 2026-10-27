use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use pack_compile_input::texture_set::{TextureSetLayer, parse_texture_set};
use serde_json::Value;

use super::{PbrFormat, PbrPack, PbrSurface, PbrTexture, PbrTextureSource, decode};
use assets::texture::{PBR_REF_COLOR, PBR_REF_NORMAL};

mod names;
pub(super) mod terrain;

const IMAGE_BYTES_LIMIT: u64 = 64 * 1024 * 1024;
const JSON_BYTES_LIMIT: u64 = 1024 * 1024;

/// Reads an explicit pack material-format declaration when one is present.
pub(super) fn declared_format(root: &Path) -> PbrFormat {
    for relative in [
        "assets/minecraft/optifine/texture.properties",
        "optifine/texture.properties",
    ] {
        if let Ok(text) = fs::read_to_string(root.join(relative)) {
            for line in text.lines().map(str::trim) {
                if let Some((key, value)) = line.split_once('=')
                    && key.trim() == "format"
                {
                    return PbrFormat::parse(value.trim()).unwrap_or_default();
                }
            }
        }
    }
    PbrFormat::Unspecified
}

/// Rejects absolute paths and parent traversal in authored image references.
fn safe_relative(value: &str) -> Option<PathBuf> {
    let value = value.replace('\\', "/");
    let path = Path::new(&value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(path.to_owned())
}

/// Image formats an authored map may use, in lookup priority order.
pub(super) const IMAGE_EXTENSIONS: [&str; 4] = ["tga", "png", "jpg", "jpeg"];

/// Builds supported image paths within the defining pack.
fn candidates(pack: &PbrPack, alias: &str, suffix: &str, extension: &str) -> Vec<PathBuf> {
    names::variants(alias)
        .into_iter()
        .filter_map(|name| {
            let relative = safe_relative(&format!("{name}{suffix}.{extension}"))?;
            Some([
                pack.root.join(&relative),
                pack.root.join("assets/minecraft").join(relative),
            ])
        })
        .flatten()
        .collect()
}

/// Finds the first supported image matching an alias and suffix.
fn find(pack: &PbrPack, alias: &str, suffix: &str) -> Option<PathBuf> {
    IMAGE_EXTENSIONS
        .into_iter()
        .flat_map(|extension| candidates(pack, alias, suffix, extension))
        .find(|path| path.is_file())
}

/// Finds texture-set metadata for a supported alias.
fn json_path(pack: &PbrPack, alias: &str) -> Option<PathBuf> {
    candidates(pack, alias, ".texture_set", "json")
        .into_iter()
        .find(|path| path.is_file())
}

/// Decodes a size-bounded image with the required channel count.
fn image(path: &Path, channels: &[u8]) -> Result<RgbaImage, String> {
    if fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len()
        > IMAGE_BYTES_LIMIT
    {
        return Err(format!(
            "{} exceeds authored texture byte limit",
            path.display()
        ));
    }
    let image = image::ImageReader::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !channels.contains(&image.color().channel_count()) {
        return Err(format!(
            "{} has unsupported {}-channel data",
            path.display(),
            image.color().channel_count()
        ));
    }
    Ok(DynamicImage::into_rgba8(image))
}

/// Creates a single-texel image for a uniform material channel.
fn solid(pixel: [u8; 4]) -> RgbaImage {
    ImageBuffer::from_pixel(1, 1, Rgba(pixel))
}

/// Resolves a uniform or image layer within its defining pack.
fn layer(
    pack: &PbrPack,
    json: &Path,
    value: &TextureSetLayer,
    allowed: &[u8],
    source: &mut Option<PathBuf>,
) -> Result<RgbaImage, String> {
    let reference = match value {
        TextureSetLayer::Uniform(pixel) => return Ok(solid(*pixel)),
        TextureSetLayer::Image(reference) => reference.as_ref(),
    };
    let relative = safe_relative(reference)
        .ok_or_else(|| "texture set image reference escapes its pack".to_owned())?;
    let stem = relative.with_extension("");
    let parent = json.parent().unwrap_or(&pack.root);
    let roots = if reference.starts_with("textures/") {
        vec![pack.root.clone(), pack.root.join("assets/minecraft")]
    } else {
        vec![parent.to_owned()]
    };
    let path = IMAGE_EXTENSIONS
        .into_iter()
        .flat_map(|extension| {
            roots
                .iter()
                .map(|root| root.join(stem.with_extension(extension)))
                .collect::<Vec<_>>()
        })
        .find(|path| path.is_file())
        .ok_or_else(|| format!("texture set cannot find {reference} in its defining pack"))?;
    *source = Some(path.clone());
    image(&path, allowed)
}

/// Reads bounded texture metadata using the shared JSONC parser.
fn read_json(path: &Path) -> Result<Value, String> {
    if fs::metadata(path).map_err(|error| error.to_string())?.len() > JSON_BYTES_LIMIT {
        return Err("texture metadata exceeds byte limit".to_owned());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    pack_json::parse_jsonc(&bytes).map_err(|error| error.to_string())
}

/// Decodes a texture set validated by the shared Bedrock schema.
fn texture_set(pack: &PbrPack, path: &Path) -> Result<PbrTexture, String> {
    let set = parse_texture_set(&read_json(path)?)
        .map_err(|error| format!("texture set is invalid: {error:?}"))?;
    let mut source = PbrTextureSource {
        color: None,
        normal: None,
        material: None,
        height: None,
        native_color_size: [0; 2],
        native_normal_size: [0; 2],
        native_material_size: [0; 2],
        color_style: pack.color_style,
    };
    let color = layer(pack, path, &set.color, &[3, 4], &mut source.color)?;
    source.native_color_size = [color.width(), color.height()];
    let mut flags = PBR_REF_COLOR;
    let normal = if let Some(reference) = &set.normal {
        flags |= PBR_REF_NORMAL;
        let (normal, normal_flags) = decode::normal(
            layer(
                pack,
                path,
                &TextureSetLayer::Image(reference.clone()),
                &[3, 4],
                &mut source.normal,
            )?,
            PbrFormat::Legacy,
            pack.normal_format,
        );
        flags |= normal_flags;
        normal
    } else {
        solid([128, 128, 255, 128])
    };
    let material = match &set.material {
        Some(value) => {
            let subsurface = set.material_has_subsurface;
            let allowed: &[u8] = if subsurface { &[4] } else { &[3, 4] };
            let (material, material_flags) = decode::mer(
                layer(pack, path, value, allowed, &mut source.material)?,
                subsurface,
            );
            flags |= material_flags;
            material
        }
        None => solid([0, 0, 255, 0]),
    };
    let height = set
        .heightmap
        .as_ref()
        .map(|value| layer(pack, path, value, &[1], &mut source.height))
        .transpose()?;
    source.native_normal_size = [normal.width(), normal.height()];
    source.native_material_size = [material.width(), material.height()];
    Ok(PbrTexture {
        surface: PbrSurface {
            color,
            normal,
            material,
            flags,
        },
        timeline: Vec::new(),
        height,
        source,
        terrain_override: false,
    })
}

/// Reads supported frame indices from bounded animation metadata.
fn animation(path: &Path) -> Result<Vec<u32>, String> {
    let meta = PathBuf::from(format!("{}.mcmeta", path.display()));
    if !meta.is_file() {
        return Ok(Vec::new());
    }
    let json = read_json(&meta)?;
    let Some(frames) = json
        .get("animation")
        .and_then(|value| value.get("frames"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let default_time = json
        .get("animation")
        .and_then(|value| value.get("frametime"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let mut timeline = Vec::new();
    for frame in frames {
        let index = frame
            .as_u64()
            .or_else(|| frame.get("index").and_then(Value::as_u64))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| "invalid authored animation frame index".to_owned())?;
        let repeat = frame
            .get("time")
            .and_then(Value::as_u64)
            .unwrap_or(default_time)
            .min(512) as usize;
        if repeat == 0 || timeline.len().saturating_add(repeat) > 4096 {
            return Err("authored animation timeline exceeds supported budget".to_owned());
        }
        timeline.extend(std::iter::repeat_n(index, repeat));
    }
    Ok(timeline)
}

/// Applies the shared atlas-space multiplier before frame extraction and mip filtering.
fn tinted(mut texture: PbrTexture, terrain: Option<&terrain::TerrainSource>) -> PbrTexture {
    texture.terrain_override = terrain.is_some();
    if let Some(tint) = terrain.and_then(|source| source.tint) {
        pack_compile_input::apply_atlas_tint(texture.surface.color.as_mut(), tint);
    }
    texture
}

/// Resolves aliases and companion maps while respecting explicit material formats.
pub(super) fn load(
    packs: &[PbrPack],
    alias: &str,
    key: Option<&str>,
) -> Result<Option<PbrTexture>, String> {
    let prefer_complete = packs.iter().any(PbrPack::prefers_complete_material);
    let mut fallback = None;
    for owner in packs {
        let terrain = match key {
            Some(key) => owner.terrain_key_source(key)?,
            None => owner.terrain_source(alias)?,
        };
        let alias = terrain.map_or(alias, |source| source.path.as_str());
        if let Some(path) = json_path(owner, alias) {
            let texture = tinted(texture_set(owner, &path)?, terrain);
            if !prefer_complete || texture.has_complete_material() {
                return Ok(Some(texture));
            }
            if fallback.is_none() {
                fallback = Some(texture);
            }
            continue;
        }
        let Some(color_path) = find(owner, alias, "") else {
            continue;
        };
        let color = image(&color_path, &[3, 4])?;
        let map_alias = color_path
            .strip_prefix(owner.root.join("assets/minecraft"))
            .or_else(|_| color_path.strip_prefix(&owner.root))
            .map_err(|_| "authored color path escapes its pack")?
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let companions = std::iter::once(owner).chain(packs.iter().filter(|pack| {
            pack.root != owner.root
                && owner
                    .group
                    .as_ref()
                    .is_some_and(|group| pack.group.as_ref() == Some(group))
        }));
        let companions = companions.collect::<Vec<_>>();
        let mut flags = PBR_REF_COLOR;
        let normal_source = companions.iter().find_map(|pack| {
            find(pack, &map_alias, "_normal")
                .filter(|path| *path != color_path)
                .map(|path| (*pack, path, PbrFormat::Legacy))
                .or_else(|| {
                    effective_format(owner, pack).and_then(|format| {
                        find(pack, &map_alias, "_n")
                            .filter(|path| *path != color_path)
                            .map(|path| (*pack, path, format))
                    })
                })
        });
        let normal = if let Some((pack, path, format)) = &normal_source {
            let (normal, normal_flags) =
                decode::normal(image(path, &[3, 4])?, *format, pack.normal_format);
            flags |= PBR_REF_NORMAL | normal_flags;
            normal
        } else {
            solid([128, 128, 255, 128])
        };
        let material_source = companions.iter().find_map(|pack| {
            find(pack, &map_alias, "_mers")
                .map(|path| (path, true, None))
                .or_else(|| find(pack, &map_alias, "_mer").map(|path| (path, false, None)))
                .or_else(|| {
                    effective_format(owner, pack).and_then(|format| {
                        find(pack, &map_alias, "_s").map(|path| (path, false, Some(format)))
                    })
                })
        });
        let material = if let Some((path, subsurface, format)) = &material_source {
            let allowed: &[u8] = if format.is_some() {
                &[1, 2, 3, 4]
            } else if *subsurface {
                &[4]
            } else {
                &[3, 4]
            };
            let raw = image(path, allowed)?;
            let (material, material_flags) = match format {
                Some(format) => decode::specular(raw, *format),
                None => decode::mer(raw, *subsurface),
            };
            flags |= material_flags;
            material
        } else {
            solid([0, 0, 255, 0])
        };
        let height_path = companions.iter().find_map(|pack| {
            find(pack, &map_alias, "_h").or_else(|| find(pack, &map_alias, "_heightmap"))
        });
        let height = height_path
            .as_ref()
            .map(|path| image(path, &[1, 2, 3, 4]))
            .transpose()?;
        let source = PbrTextureSource {
            color: Some(color_path.clone()),
            normal: normal_source.map(|(_, path, _)| path),
            material: material_source.map(|(path, _, _)| path),
            height: height_path,
            native_color_size: [color.width(), color.height()],
            native_normal_size: [normal.width(), normal.height()],
            native_material_size: [material.width(), material.height()],
            color_style: owner.color_style,
        };
        let texture = tinted(
            PbrTexture {
                surface: PbrSurface {
                    color,
                    normal,
                    material,
                    flags,
                },
                timeline: animation(&color_path)?,
                height,
                source,
                terrain_override: false,
            },
            terrain,
        );
        if !prefer_complete || texture.has_complete_material() {
            return Ok(Some(texture));
        }
        if fallback.is_none() {
            fallback = Some(texture);
        }
    }
    Ok(fallback)
}

/// A declared format applies to a grouped material set, so a companion pack
/// may contain only maps while the color owner carries the metadata. An
/// unspecified pair still rejects Java `_n`/`_s` guesses.
fn effective_format(owner: &PbrPack, companion: &PbrPack) -> Option<PbrFormat> {
    [companion.format, owner.format]
        .into_iter()
        .find(|format| *format != PbrFormat::Unspecified)
}
