//! `cinnabar_hud_control` panel nodes: named controls of the same mod's HUD surface, centred
//! in the node and fitted by their laid-out size, by a fixed scale, or by their painted ink.

use std::collections::BTreeMap;

use json_ui::{Draw, DrawNode, ViewState};
use serde_json::Value;
use ui::{
    IconRef,
    mod_panel::hud_control::{self, BOUND_CONTROL, CONTROL, Fitting, control_name},
};
use ui_engine::engine::{
    CachedFragment, Canvas, FormEngine,
    screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame},
};

use super::HudSurfaces;
use crate::{PanelUiOwner, local_player_identity::LocalPlayerIdentity};

/// The panel nodes drawing embedded controls on the last prepared frame.
#[derive(Default)]
pub(crate) struct Embeds {
    nodes: BTreeMap<String, Instance>,
}

struct Instance {
    control: String,
    live: bool,
    replay: CachedFragment,
    fit: FitCache,
}

/// Everything an ink fit depends on.
#[derive(Clone, Copy, PartialEq)]
struct FitKey {
    /// The painted ink's size, in the part's logical pixels.
    ink: [f32; 2],
    /// The node's size in canvas logical pixels.
    node: [f32; 2],
    /// Canvas logical pixels per logical pixel of the part.
    ui_scale: f32,
    fraction: f32,
    max_scale: f32,
}

/// The last ink fit of one node.
#[derive(Default)]
struct FitCache {
    value: Option<(FitKey, f32)>,
    /// Fits computed, for cache tests.
    #[cfg(test)]
    computations: usize,
}

impl FitCache {
    /// The multiple of the control's HUD size at which its ink fills `fraction` of the node
    /// along the limiting axis, at most `max_scale`; unchanged sizes and parameters reuse it.
    fn scale(&mut self, key: FitKey) -> f32 {
        if let Some((previous, scale)) = self.value
            && previous == key
        {
            return scale;
        }
        let scale = ((key.node[0] / key.ink[0]).min(key.node[1] / key.ink[1]) * key.fraction
            / key.ui_scale)
            .min(key.max_scale);
        self.value = Some((key, scale));
        #[cfg(test)]
        {
            self.computations += 1;
        }
        scale
    }
}

/// The control a node names: its bound selector, else its literal `control`.
fn selected(data: &BTreeMap<String, Value>) -> Option<&str> {
    data.get(BOUND_CONTROL)
        .or_else(|| data.get(CONTROL))
        .and_then(Value::as_str)
        .filter(|name| control_name(name))
}

impl Embeds {
    /// Prepares the controls of shown embedding nodes after the panel's layout.
    #[expect(
        clippy::too_many_arguments,
        reason = "preparation borrows the panel layout, owner, layers, engine, frame and atlas"
    )]
    pub(crate) fn prepare(
        &mut self,
        nodes: &[DrawNode],
        view: &ViewState,
        owner: PanelUiOwner,
        surfaces: &mut HudSurfaces,
        identity: &LocalPlayerIdentity,
        engine: &FormEngine,
        frame: &mut PaintFrame<'_>,
        icon: &dyn Fn(&str, u32) -> Option<IconRef>,
    ) {
        for instance in self.nodes.values_mut() {
            instance.live = false;
        }
        // A panel opened before any HUD frame lays its embeds out against the HUD root this
        // frame would place.
        if !surfaces.root_size().iter().all(|axis| *axis > 0.) {
            surfaces.place(frame);
        }
        let canvas = surfaces.root_size();
        if let Some(layer) = surfaces.layer_mut(owner) {
            layer.begin_embeds();
        }
        for node in nodes {
            if (node.alpha <= 0. && node.anim.is_none()) || !node.shown(view) {
                continue;
            }
            let Draw::Custom { renderer, data } = &node.draw else {
                continue;
            };
            if renderer != hud_control::RENDERER {
                continue;
            }
            let Some(control) = selected(data) else {
                continue;
            };
            let Some(part) = surfaces
                .layer_mut(owner)
                .and_then(|layer| layer.embedded(control))
            else {
                continue;
            };
            part.write_identity(identity);
            if let Err(error) = part.prepare(engine, frame, canvas, icon, true) {
                tracing::warn!(%error, control, "embedded HUD control preparation failed");
                continue;
            }
            let instance = self
                .nodes
                .entry(node.key.clone())
                .or_insert_with(|| Instance {
                    control: control.to_owned(),
                    live: true,
                    replay: CachedFragment::default(),
                    fit: FitCache::default(),
                });
            if instance.control != control {
                instance.control = control.to_owned();
                instance.replay = CachedFragment::default();
            }
            instance.live = true;
        }
        self.nodes.retain(|_, instance| instance.live);
    }

    /// Ink fits computed across this panel's nodes, for cache tests.
    #[cfg(test)]
    pub(crate) fn fit_computations(&self) -> usize {
        self.nodes
            .values()
            .map(|instance| instance.fit.computations)
            .sum()
    }

    /// Borrows the prepared controls for the panel's draw.
    pub(crate) fn renderer<'a>(
        &'a mut self,
        owner: PanelUiOwner,
        surfaces: &'a mut HudSurfaces,
    ) -> Renderer<'a> {
        Renderer {
            embeds: self,
            owner,
            surfaces,
        }
    }
}

pub(crate) struct Renderer<'a> {
    embeds: &'a mut Embeds,
    owner: PanelUiOwner,
    surfaces: &'a mut HudSurfaces,
}

impl CustomRenderer for Renderer<'_> {
    /// Sizes the control as the node's [`Fitting`] asks and centres it, its laid-out box or,
    /// for an ink fit, its painted ink.
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != hud_control::RENDERER {
            return Ok(false);
        }
        let Some(instance) = self.embeds.nodes.get_mut(node.key) else {
            return Ok(true);
        };
        let Some(part) = self
            .surfaces
            .layers
            .iter()
            .find(|layer| layer.owner == self.owner)
            .and_then(|layer| layer.embedded.get(&instance.control))
            .and_then(|embedded| embedded.part.as_ref())
        else {
            return Ok(true);
        };
        let Some(fitting) = Fitting::read(|name| node.data.get(name)) else {
            return Ok(true);
        };
        let px = canvas.px();
        let node_size = [node.dest[2] - node.dest[0], node.dest[3] - node.dest[1]];
        // The centred box's top left and size, in the part's logical pixels at scale 1.
        let (scale, from, size) = match fitting {
            Fitting::Ink {
                fraction,
                max_scale,
            } => {
                let Some(ink) = part.bounds else {
                    return Ok(true);
                };
                let size = [ink.width(), ink.height()];
                let scale = instance.fit.scale(FitKey {
                    ink: size,
                    node: node_size,
                    ui_scale: px / part.px,
                    fraction,
                    max_scale,
                });
                (scale, [ink.min().x(), ink.min().y()], size)
            }
            Fitting::Layout | Fitting::Scale(_) => {
                let Some([x, y, width, height]) =
                    part.rect.filter(|rect| rect[2] > 0. && rect[3] > 0.)
                else {
                    return Ok(true);
                };
                let scale = match fitting {
                    Fitting::Scale(scale) => scale,
                    _ => (node_size[0] / (width as f32 * px))
                        .min(node_size[1] / (height as f32 * px)),
                };
                let gui = [x, y, width, height].map(|axis| axis as f32 * part.px);
                (scale, [gui[0], gui[1]], [gui[2], gui[3]])
            }
        };
        if !scale.is_finite() || scale <= 0. {
            return Ok(true);
        }
        let ratio = px / part.px * scale;
        let origin: [f32; 2] = std::array::from_fn(|axis| {
            node.dest[axis] + (node_size[axis] - size[axis] * ratio) * 0.5 - from[axis] * ratio
        });
        let opacity = f32::from(node.alpha([255; 4])[3]) / 255.;
        part.replay(
            &mut instance.replay,
            canvas,
            origin,
            scale,
            node.dest,
            opacity,
        )?;
        Ok(true)
    }

    /// Pins every embedded control's textures before replay.
    fn textures<'s>(&'s self, _: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        let Some(layer) = self.surfaces.layers.iter().find(|l| l.owner == self.owner) else {
            return;
        };
        for instance in self.embeds.nodes.values() {
            if let Some(part) = layer
                .embedded
                .get(&instance.control)
                .and_then(|embedded| embedded.part.as_ref())
            {
                for path in part.textures.paths() {
                    texture(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
