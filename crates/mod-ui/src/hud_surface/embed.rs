//! `cinnabar_hud_control` panel nodes: named controls of the same mod's HUD surface, fitted by
//! their laid-out size so authored padding frames them, and centred in the node.

use std::collections::BTreeMap;

use json_ui::{Draw, DrawNode, ViewState};
use serde_json::Value;
use ui::{
    IconRef,
    mod_panel::hud_control::{self, BOUND_CONTROL, CONTROL, control_name},
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
                });
            if instance.control != control {
                instance.control = control.to_owned();
                instance.replay = CachedFragment::default();
            }
            instance.live = true;
        }
        self.nodes.retain(|_, instance| instance.live);
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
    /// Fits the control's laid-out size uniformly inside the node and centres it.
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
        let Some([x, y, width, height]) = part.rect.filter(|rect| rect[2] > 0. && rect[3] > 0.)
        else {
            return Ok(true);
        };
        let px = canvas.px();
        let node_size = [node.dest[2] - node.dest[0], node.dest[3] - node.dest[1]];
        let scale = (node_size[0] / (width as f32 * px)).min(node_size[1] / (height as f32 * px));
        if !scale.is_finite() || scale <= 0. {
            return Ok(true);
        }
        let origin = [
            node.dest[0] + (node_size[0] - width as f32 * px * scale) * 0.5 - x as f32 * px * scale,
            node.dest[1] + (node_size[1] - height as f32 * px * scale) * 0.5
                - y as f32 * px * scale,
        ];
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
