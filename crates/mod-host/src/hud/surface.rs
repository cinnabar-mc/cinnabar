//! The mod's JSON-UI HUD surface, the HUD root it lays out against, text metrics and vanilla
//! element state and hiding.

use super::super::State;
use super::super::cinnabar::extension::hud as wit;
use super::admit;
use anyhow::Result;
use std::sync::Arc;
use ui::mod_hud::{HiddenElements, VanillaElement};
use ui::mod_panel::{HudIndex, LabelFont, Surface, SurfacePatch};

/// Largest `set-surface` JSON: a full document, 512 bindings and collection metadata.
pub const MAX_HUD_SURFACE_JSON_BYTES: usize = 384 * 1024;
/// Text measurements one callback may make.
pub const MAX_TEXT_MEASURES: u32 = 256;
/// Longest text one measurement accepts.
pub const MAX_MEASURED_TEXT_BYTES: usize = 1024;

/// Stages a validated surface, or its removal for empty text.
pub(in super::super) fn set(state: &mut State, json: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if json.is_empty() {
        state.hud.pending_surface = Some(None);
        state.hud.index = HudIndex::default();
        return Ok(Ok(()));
    }
    if json.len() > MAX_HUD_SURFACE_JSON_BYTES {
        return Ok(Err("HUD surface exceeds its byte limit".into()));
    }
    let surface: Surface = match serde_json::from_str(&json) {
        Ok(surface) => surface,
        Err(error) => return Ok(Err(format!("invalid HUD surface: {error}"))),
    };
    let index = match HudIndex::new(&surface)
        .and_then(|index| actor_authority(state, &surface).map(|()| index))
    {
        Ok(index) => index,
        Err(error) => return Ok(Err(error)),
    };
    state.hud.index = index;
    state.hud.pending_surface = Some(Some(Arc::new(surface)));
    Ok(Ok(()))
}

/// Patches the staged or committed surface's bindings, including removals, collection order
/// and control definitions; the last admitted document's index limits a control patch's
/// revalidation to what it changes.
pub(in super::super) fn update(state: &mut State, json: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    let patch = match SurfacePatch::parse(&json) {
        Ok(patch) => patch,
        Err(error) => return Ok(Err(error)),
    };
    let current = match &state.hud.pending_surface {
        Some(pending) => pending.as_ref(),
        None => state.hud.surface.as_ref(),
    };
    let Some(current) = current else {
        return Ok(Err("no HUD surface to update".into()));
    };
    let (next, index) = match patch
        .apply_hud(current, &state.hud.index)
        .and_then(|patched| actor_authority(state, &patched.0).map(|()| patched))
    {
        Ok(patched) => patched,
        Err(error) => return Ok(Err(error)),
    };
    state.hud.index = index;
    state.hud.pending_surface = Some(Some(Arc::new(next)));
    Ok(Ok(()))
}

/// Projections that follow an actor need actor observation authority, in HUD and panel
/// surfaces alike.
pub(in super::super) fn actor_authority(state: &State, surface: &Surface) -> Result<(), String> {
    let follows_actor = surface
        .bindings
        .values()
        .any(|value| value.projection().and_then(|p| p.actor()).is_some());
    if follows_actor && !state.grants.players && !state.grants.entities {
        return Err("actor projections require players or entities authority".into());
    }
    Ok(())
}

/// The last laid-out HUD root, for hud-granted mods.
pub(in super::super) fn viewport(state: &State) -> Option<wit::Viewport> {
    let viewport = state.hud.viewport.filter(|_| state.grants.hud)?;
    Some(wit::Viewport {
        width: viewport.width,
        height: viewport.height,
        scale: viewport.scale,
    })
}

/// Measures one line through the host's label metrics, within a per-callback budget.
pub(in super::super) fn measure(
    state: &mut State,
    text: &str,
    font: wit::Font,
) -> Result<f32, String> {
    if !state.grants.hud {
        return Err("HUD capability denied".into());
    }
    state.hud.measures += 1;
    if state.hud.measures > MAX_TEXT_MEASURES {
        return Err("text measurement budget exhausted".into());
    }
    if text.len() > MAX_MEASURED_TEXT_BYTES || text.chars().any(char::is_control) {
        return Err("measured text must be one bounded line".into());
    }
    let font = match font {
        wit::Font::Native => LabelFont::Native,
        wit::Font::Sans => LabelFont::Sans,
    };
    state
        .hud
        .text
        .as_ref()
        .and_then(|text_measure| text_measure.width(text, font))
        .filter(|width| width.is_finite() && *width >= 0.)
        .ok_or_else(|| "text metrics are unavailable".into())
}

/// Stages the retained set of hidden vanilla elements.
pub(in super::super) fn set_hidden(
    state: &mut State,
    elements: Vec<wit::VanillaElement>,
) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    state.hud.pending_hidden = Some(HiddenElements::from_elements(
        elements.into_iter().map(element),
    ));
    Ok(Ok(()))
}

/// Every vanilla element's last observed state, in enum order.
pub(in super::super) fn elements(state: &State) -> Result<Vec<wit::ElementState>, String> {
    if !state.grants.hud {
        return Err("HUD capability denied".into());
    }
    Ok(VanillaElement::ALL
        .into_iter()
        .map(|kind| {
            let observed = state.hud.elements[kind.index()];
            wit::ElementState {
                element: wire(kind),
                visible: observed.visible,
                bounds: observed.bounds.map(|[x, y, width, height]| wit::Rect {
                    x,
                    y,
                    width,
                    height,
                }),
            }
        })
        .collect())
}

/// The host element a WIT element names.
fn element(element: wit::VanillaElement) -> VanillaElement {
    match element {
        wit::VanillaElement::Crosshair => VanillaElement::Crosshair,
        wit::VanillaElement::StatusEffects => VanillaElement::StatusEffects,
        wit::VanillaElement::Sidebar => VanillaElement::Sidebar,
        wit::VanillaElement::Hunger => VanillaElement::Hunger,
    }
}

/// The WIT element naming a host element.
fn wire(element: VanillaElement) -> wit::VanillaElement {
    match element {
        VanillaElement::Crosshair => wit::VanillaElement::Crosshair,
        VanillaElement::StatusEffects => wit::VanillaElement::StatusEffects,
        VanillaElement::Sidebar => wit::VanillaElement::Sidebar,
        VanillaElement::Hunger => wit::VanillaElement::Hunger,
    }
}
