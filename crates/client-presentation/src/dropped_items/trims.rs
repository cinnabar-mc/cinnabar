//! Trimmed armor sprites, which vanilla draws wherever the stack's icon is drawn.
use super::*;

/// Admits a trimmed armor sprite, measuring and composing it only when it is not resident.
pub(super) fn trimmed_model(
    cache: &mut ModelCache,
    icons: &UiPresentationRuntime,
    artwork: client_ui::ui_runtime::presentation::gui_models::WorldTrimArtwork,
) -> Option<u32> {
    cache.model(
        ModelKey::Trim(artwork.clone()),
        || artwork.cost(icons, item_sprite_side()),
        || artwork.model(icons, item_sprite_side()),
    )
}

/// A framed trimmed armor stack draws its trimmed icon; `None` leaves the plain icon route.
pub(super) fn placed_trim_model(
    cache: &mut ModelCache,
    icons: &UiPresentationRuntime,
    placement: &render_model::StaticItemPlacement,
    icon: ui::IconRef,
) -> Option<u32> {
    let artwork =
        icons.world_trim_artwork(&placement.identifier, icon, placement.trim.as_ref(), false)?;
    trimmed_model(cache, icons, artwork)
}
