//! Server WIT 0.10's overlays: the blocks that may carry them, and overlay types, each checked as
//! a block's states, visual and permutations are. The adapter checks the same rules again before
//! it registers them.

use std::collections::HashSet;

use anyhow::{Context, Result, bail, ensure};

use super::block_type::{self, Assets};
use super::{axes, is_block_name};
use crate::host::cinnabar::experience_server::types as wit;
use crate::limits::{MAX_BLOCK_NAME_BYTES, MAX_OVERLAY_TYPES};
use crate::protocol::{BlockDef, OverlayDef, OverlayRegistration};

/// Checks what `overlays` declared against the Experience's validated `blocks`: hosts that are
/// its blocks, each once, and at most [`MAX_OVERLAY_TYPES`] overlay types, each named by the
/// block-name rule in the blocks' namespace without taking a block's or another type's id.
pub(super) fn validate(
    assets: &mut Assets<'_>,
    blocks: &[BlockDef],
    registration: wit::OverlayRegistration,
) -> Result<OverlayRegistration> {
    let wit::OverlayRegistration { hosts, types } = registration;
    let mut seen = HashSet::with_capacity(hosts.len());
    for host in &hosts {
        ensure!(
            blocks.iter().any(|block| block.id == *host),
            "overlay host \"{host}\" is not one of the Experience's blocks"
        );
        ensure!(
            seen.insert(host.as_str()),
            "overlay host \"{host}\" is listed twice"
        );
    }
    ensure!(
        types.len() <= MAX_OVERLAY_TYPES,
        "overlays declared {} overlay types; the limit is {MAX_OVERLAY_TYPES}",
        types.len()
    );
    let namespace = format!("{}:", assets.id);
    let mut checked: Vec<OverlayDef> = Vec::with_capacity(types.len());
    for overlay in types {
        let id = format!("{namespace}{}", overlay.name);
        ensure!(
            is_block_name(&overlay.name),
            "overlay type \"{id}\" has an invalid name: it must match \
             ^[a-z0-9_]{{1,{MAX_BLOCK_NAME_BYTES}}}$"
        );
        if blocks.iter().any(|block| block.id == id) {
            bail!("overlay type \"{id}\" has the id of a block");
        }
        ensure!(
            checked.iter().all(|earlier| earlier.id != id),
            "overlay type \"{id}\" is declared twice"
        );
        let context = format!("overlay type \"{id}\"");
        checked.push(overlay_type(assets, id, overlay).context(context)?);
    }
    Ok(OverlayRegistration {
        hosts,
        types: checked,
    })
}

/// Checks one overlay type's states, visual and permutations, as a block's without placement
/// traits.
fn overlay_type(
    assets: &mut Assets<'_>,
    id: String,
    overlay: wit::OverlayType,
) -> Result<OverlayDef> {
    let wit::OverlayType {
        name: _,
        states,
        visual,
        permutations,
    } = overlay;
    let (states, placement) = block_type::states(assets.id, states, wit::PlacementStates::empty())?;
    let axes = axes(&states, &placement);
    let look = block_type::visual(assets, &axes, visual)?;
    let permutations = block_type::permutations(assets, &axes, &look, permutations)?;
    Ok(OverlayDef {
        id,
        states,
        visual: look.visual,
        permutations,
    })
}
