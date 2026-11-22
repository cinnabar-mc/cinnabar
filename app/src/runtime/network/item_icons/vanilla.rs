//! Base-game rasters behind pack references: item carrier sprites and their source routes, and
//! world carrier terrain layers by source image path.
use super::{MAX_SESSION_ICONS, icon};
use assets::{
    ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets, material_keys::MaterialKeys,
};
use client_ui::ui_runtime::presentation::SessionIcon;
use pack_runtime::pack_textures::DecodedTexture;
use resource_pack::LayeredPackView;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

struct Route {
    identifier: Arc<str>,
    metadata: u32,
    path: String,
    variant: u32,
    aliases: Vec<String>,
}
pub(in crate::runtime::network) static CACHE_IDENTITY: OnceLock<Option<([u8; 32], [u8; 32])>> =
    OnceLock::new();

static PATHS: OnceLock<Vec<Route>> = OnceLock::new();

/// Whether the carrier's item routes are installed; once installed they never change.
pub(crate) fn vanilla_item_paths_installed() -> bool {
    PATHS.get().is_some()
}

/// Installs the item routes and matching base artwork before pack compilation.
pub(crate) fn install_vanilla_item_assets(
    entities: &RuntimeEntityAssets,
    icons: &assets::RuntimeIconCatalog,
) {
    set_vanilla_item_paths(entities);
    set_vanilla_item_sprites(entities, icons);
    let _ = CACHE_IDENTITY.set(
        entities
            .carrier_identity()
            .map(|identity| (identity, icons.carrier_identity())),
    );
}

/// Retains source routes once, so menus can apply packs before any item registry arrives.
fn set_vanilla_item_paths(entities: &RuntimeEntityAssets) {
    super::vanilla_blocks::install(entities);
    let _ = PATHS.set(routes(entities));
}

/// Each sprite-routed item variant with its base source image, atlas variant, and the short
/// names of every item drawn from that same image and variant.
fn routes(entities: &RuntimeEntityAssets) -> Vec<Route> {
    let mut aliases_by_source = HashMap::<_, Vec<String>>::new();
    for item in entities.item_visuals() {
        if let ItemVisualDefinitionRoute::Sprite { texture } = item.route {
            aliases_by_source
                .entry((texture.source, texture.variant))
                .or_default()
                .push(
                    item.key
                        .identifier
                        .strip_prefix("minecraft:")
                        .unwrap_or(&item.key.identifier)
                        .to_owned(),
                );
        }
    }
    entities
        .item_visuals()
        .iter()
        .filter_map(|item| {
            let ItemVisualDefinitionRoute::Sprite { texture } = item.route else {
                return None;
            };
            let source = entities.sources().get(texture.source as usize)?;
            let aliases = aliases_by_source
                .get(&(texture.source, texture.variant))
                .cloned()
                .unwrap_or_default();
            Some(Route {
                identifier: Arc::from(item.key.identifier.as_ref()),
                metadata: item.key.metadata,
                path: source.path.to_string(),
                variant: texture.variant,
                aliases,
            })
        })
        .collect()
}

/// Appends only files actually supplied by a pack; absent overrides keep their base icons.
pub(super) fn append(
    view: &LayeredPackView,
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
) {
    let Some(paths) = PATHS.get() else {
        return;
    };
    append_routes(view, paths, icons, retained_bytes);
}

/// Pins base art for variants a pack left alone beside its replaced variant zero; see
/// [`pin_base_variants`]. Runs last, so pins never displace other session icons, and stops at
/// `limit` icons so they never crowd out what is staged after them.
pub(super) fn pin(
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
    keyed: &HashSet<Arc<str>>,
    limit: usize,
) {
    let Some(paths) = PATHS.get() else {
        return;
    };
    pin_base_variants(paths, icons, retained_bytes, keyed, limit, sprite);
}

/// Strips a route's image extension, as pack textures are named without one.
fn stem(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(stem, _)| stem)
}

/// Appends `texture` as `route`'s icon when both session budgets allow it.
fn push_route_icon(
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
    route: &Route,
    mut texture: DecodedTexture,
) -> bool {
    texture.retain_first_strip_frame();
    let mut replacement = icon(route.identifier.clone(), texture);
    if icons.len() >= MAX_SESSION_ICONS
        || replacement.rgba8.len()
            > assets::texture::policy::MAX_SUBSCRIBER_BYTES.saturating_sub(*retained_bytes)
    {
        return false;
    }
    *retained_bytes += replacement.rgba8.len();
    replacement.metadata = route.metadata;
    icons.push(replacement);
    true
}

/// Appends each route's pack file, through the pack's own atlas entry when it names one.
fn append_routes(
    view: &LayeredPackView,
    paths: &[Route],
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
) {
    let mut present = icons
        .iter()
        .map(|icon| (icon.identifier.clone(), icon.metadata))
        .collect::<HashSet<_>>();
    let catalog = super::catalog::paths(view);
    for route in paths {
        if icons.len() >= MAX_SESSION_ICONS {
            break;
        }
        if present.contains(&(route.identifier.clone(), route.metadata)) {
            continue;
        }
        let override_path = route
            .aliases
            .iter()
            .find_map(|key| catalog.get(key)?.get(route.variant as usize));
        if let Some(texture) = pack_runtime::pack_textures::decode_pack_texture_with_policy(
            view,
            override_path.map_or(stem(&route.path), String::as_str),
            assets::texture::policy::device_policy(),
            true,
        ) && push_route_icon(icons, retained_bytes, route, texture)
        {
            present.insert((route.identifier.clone(), route.metadata));
        }
    }
}

/// Pins `base` art for every nonzero variant whose item has a variant-zero icon in `icons` but
/// no exact one, while `icons` holds fewer than `limit`. Session lookups fall back to variant
/// zero, so without the pin a pack that replaces only the plain bottle would draw it for every
/// potion. Items in `keyed` took variant zero from their icon key, an image meant to cover every
/// variant, and are never pinned; only a replaced base route file is.
fn pin_base_variants(
    paths: &[Route],
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
    keyed: &HashSet<Arc<str>>,
    limit: usize,
    base: impl Fn(&str) -> Option<DecodedTexture>,
) {
    let mut present = icons
        .iter()
        .map(|icon| (icon.identifier.clone(), icon.metadata))
        .collect::<HashSet<_>>();
    for route in paths.iter().filter(|route| route.metadata != 0) {
        if icons.len() >= limit {
            break;
        }
        if keyed.contains(&route.identifier)
            || !present.contains(&(route.identifier.clone(), 0))
            || present.contains(&(route.identifier.clone(), route.metadata))
        {
            continue;
        }
        if let Some(texture) = base(stem(&route.path))
            && push_route_icon(icons, retained_bytes, route, texture)
        {
            present.insert((route.identifier.clone(), route.metadata));
        }
    }
}

static SPRITES: OnceLock<HashMap<String, assets::IconSprite>> = OnceLock::new();

/// Retains base sprite pixels by their recorded source filenames for custom item aliases.
fn set_vanilla_item_sprites(entities: &RuntimeEntityAssets, icons: &assets::RuntimeIconCatalog) {
    if let Some(sprites) = base_sprites(entities, icons) {
        let _ = SPRITES.set(sprites);
    }
}

/// Base sprites by lowercase source filename, or `None` when the icon carrier was compiled from
/// different sources than the entity carrier's routes.
fn base_sprites(
    entities: &RuntimeEntityAssets,
    icons: &assets::RuntimeIconCatalog,
) -> Option<HashMap<String, assets::IconSprite>> {
    if entities.source_manifest_sha256() != icons.source_manifest_sha256() {
        return None;
    }
    Some(
        icons
            .sources()
            .unwrap_or_default()
            .iter()
            .filter_map(|source| {
                let sprite = icons.sprites().get(source.sprite as usize)?.clone();
                Some((source.path.to_ascii_lowercase(), sprite))
            })
            .collect(),
    )
}

/// Resolves an absent pack raster against a matching immutable base sprite filename, then the
/// base terrain raster of that filename, as the base game's pack sits beneath every pack stack.
pub(super) fn texture(path: &str) -> Option<DecodedTexture> {
    sprite(path).or_else(|| {
        let (keys, base) = super::super::resource_packs::base_material_keys()?;
        terrain_raster(
            TERRAIN_RASTERS.get_or_init(|| terrain_raster_index(keys)),
            base,
            path,
        )
    })
}

/// Copies a base item sprite whose recorded source filename matches `path`.
fn sprite(path: &str) -> Option<DecodedTexture> {
    sprite_in(SPRITES.get()?, path)
}

/// Copies the sprite in `sprites` (keyed by lowercase source filename) that `path` names.
fn sprite_in(sprites: &HashMap<String, assets::IconSprite>, path: &str) -> Option<DecodedTexture> {
    let sprite = pack_runtime::pack_textures::texture_image_paths(path)
        .into_iter()
        .find_map(|path| sprites.get(&path.to_ascii_lowercase()))?;
    Some(DecodedTexture {
        width: u32::from(sprite.width),
        height: u32::from(sprite.height),
        rgba8: sprite.rgba8.to_vec().into_boxed_slice(),
    })
}

/// Base world materials by their lowercase source image filename, built on first use.
static TERRAIN_RASTERS: OnceLock<HashMap<String, u32>> = OnceLock::new();

/// Indexes materials whose carrier layer is an exact copy of their source image, so inheriting
/// an image gives the same pixels as shipping it; the lowest id wins when states share one.
/// Larger or animated images are compiled to a downsampled tile or a selected frame instead.
fn terrain_raster_index(keys: &MaterialKeys) -> HashMap<String, u32> {
    let mut index = HashMap::new();
    for (material, source) in keys.exact_carrier_rasters() {
        index
            .entry(source.raster.to_ascii_lowercase())
            .and_modify(|id: &mut u32| *id = (*id).min(material))
            .or_insert(material);
    }
    index
}

/// Copies the first-mip base terrain layer compiled from `path`'s image.
fn terrain_raster(
    index: &HashMap<String, u32>,
    base: &RuntimeAssets,
    path: &str,
) -> Option<DecodedTexture> {
    let material = pack_runtime::pack_textures::texture_image_paths(path)
        .into_iter()
        .find_map(|path| index.get(&path.to_ascii_lowercase()).copied())?;
    let ([width, height], rgba8) = base.material_texels(material)?;
    Some(DecodedTexture {
        width,
        height,
        rgba8,
    })
}

#[cfg(test)]
mod tests;
