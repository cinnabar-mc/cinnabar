//! A pack's own compass and clock strips, carried as one session icon per frame so the dials
//! animate with the pack's artwork instead of the base game's.

use super::MAX_SESSION_ICONS;
use assets::{
    icon::dial::{DIAL_STRIPS, DialStrip},
    texture::policy::{MAX_SUBSCRIBER_BYTES, device_policy},
};
use client_ui::ui_runtime::presentation::SessionIcon;
use pack_runtime::pack_textures::decode_pack_texture_with_policy;
use resource_pack::LayeredPackView;

/// Appends the native frames of every dial strip `view` supplies, each frame under its strip's
/// frames identifier with the frame index as metadata. A strip is added whole or not at all,
/// within the icon count and retained pixel budgets, so frames never mix with the base game's.
pub(super) fn append(
    view: &LayeredPackView,
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
) {
    for strip in &DIAL_STRIPS {
        let Some(frames) = pack_frames(view, strip) else {
            continue;
        };
        let bytes: usize = frames.iter().map(|frame| frame.rgba8.len()).sum();
        if icons.len() + frames.len() > MAX_SESSION_ICONS
            || bytes > MAX_SUBSCRIBER_BYTES.saturating_sub(*retained_bytes)
        {
            continue;
        }
        *retained_bytes += bytes;
        icons.extend(frames);
    }
}

/// The native frames of the strip `view` supplies for `strip`: its atlas, else the same strip
/// under the Java item name, which a pack carried over from Java Edition keeps.
fn pack_frames(view: &LayeredPackView, strip: &DialStrip) -> Option<Vec<SessionIcon>> {
    let atlas = strip.strip_path.trim_end_matches(".png").to_owned();
    let named = format!(
        "textures/items/{}",
        strip.item.strip_prefix("minecraft:").unwrap_or(strip.item)
    );
    [(atlas, false), (named, true)]
        .into_iter()
        .find_map(|(path, exact)| {
            let texture = decode_pack_texture_with_policy(view, &path, device_policy(), true)?;
            let side =
                inventory::strip_frame_side(strip.kind, texture.width, texture.height, exact)?;
            let frame_bytes = side as usize * side as usize * 4;
            let count = inventory::strip_frames(strip.kind) as usize;
            let pixels = texture.rgba8.get(..frame_bytes * count)?;
            Some(
                pixels
                    .chunks_exact(frame_bytes)
                    .enumerate()
                    .map(|(frame, rgba8)| SessionIcon {
                        identifier: strip.frames_identifier.into(),
                        metadata: frame as u32,
                        width: side,
                        height: side,
                        rgba8: rgba8.into(),
                    })
                    .collect(),
            )
        })
}
