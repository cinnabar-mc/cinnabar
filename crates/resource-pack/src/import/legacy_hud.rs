//! Individual HUD textures for packs that draw their HUD from the legacy atlases.

use crate::{MAX_FILE_BYTES, MAX_PACK_TEXTURE_BYTES};
use std::{collections::HashMap, io::Cursor};
use zip::ZipArchive;

/// The legacy atlas holding the heart, hunger, armour and experience sprites.
const ICONS: &str = "textures/gui/icons.png";
/// The legacy atlas holding the hotbar strip and its selection.
const GUI: &str = "textures/gui/gui.png";
/// Image formats a pack may use for a texture that `.png` also names.
const OTHER_IMAGE_EXTENSIONS: [&str; 3] = [".tga", ".jpg", ".jpeg"];

/// The HUD textures a pack's legacy atlases describe and the pack does not provide itself.
/// `kept` maps each lowercase pack path to its archive index. An atlas that cannot be read
/// contributes nothing, so a malformed one never blocks the import.
pub(super) fn derive(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    kept: &HashMap<String, usize>,
) -> Vec<(String, Vec<u8>)> {
    let mut read = |path: &str| {
        let index = *kept.get(path)?;
        super::read_entry(archive, index, MAX_FILE_BYTES).ok()
    };
    let (icons, gui) = (read(ICONS), read(GUI));
    if icons.is_none() && gui.is_none() {
        return Vec::new();
    }
    let has = |path: &str| {
        let path = path.to_ascii_lowercase();
        kept.contains_key(&path)
            || path.strip_suffix(".png").is_some_and(|stem| {
                OTHER_IMAGE_EXTENSIONS
                    .iter()
                    .any(|extension| kept.contains_key(&format!("{stem}{extension}")))
            })
    };
    java_resource_pack::legacy_hud_textures(
        icons.as_deref(),
        gui.as_deref(),
        &has,
        MAX_PACK_TEXTURE_BYTES,
    )
}
