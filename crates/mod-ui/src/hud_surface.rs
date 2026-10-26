//! Mods' JSON-UI layers over gameplay. Independent top-level controls are laid out, painted
//! and cached on their own; layered descendants and collection grids stay with their root. A binding change repaints the
//! controls that read it, a replaced definition rebuilds only the controls and embedded
//! controls whose resolution reads it, and a projected `#offset` that alone moves a control
//! moves its cached artwork. Several mods' layers draw in load order, each under its own budget.

mod document;
pub(crate) mod embed;
mod layer;
mod part;
#[cfg(test)]
mod redefine_tests;
#[cfg(test)]
mod tests;
mod values;

use std::sync::Arc;

use json_ui::{Catalog, ViewState};
use ui::{IconRef, mod_hud::HudViewport, mod_panel::Surface};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        Canvas, FormEngine, ScreenArt,
        screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame},
    },
};

use crate::{ModUi, PanelUiOwner};
use layer::Layer;
pub use values::{ActorAuthority, ProjectedActor};
pub(crate) use values::{RootPlacement, project, scalar};

const SCREEN: &str = "cinnabar_hud_surfaces.root";
const RENDERER: &str = "cinnabar_hud_surface_layers";
const TEMPLATE: &[u8] = br#"{"namespace":"cinnabar_hud_surfaces","root":{
    "type":"panel","size":["100%","100%"],
    "controls":[{"layers":{"type":"custom","renderer":"cinnabar_hud_surface_layers",
        "size":["100%","100%"],"anchor_from":"top_left","anchor_to":"top_left"}}]}}"#;

/// One mod's committed surface as the presenter receives it.
pub struct PublishedHudSurface<'a> {
    pub owner: PanelUiOwner,
    /// Changes whenever the surface or its bindings change.
    pub revision: u64,
    pub surface: &'a Arc<Surface>,
    pub authority: ActorAuthority,
}

/// Every mod's HUD layer and the replay screen that draws them.
#[derive(Default)]
pub(crate) struct HudSurfaces {
    layers: Vec<Layer>,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    /// The replay screen's empty data, shared so steady frames allocate nothing.
    data: Arc<json_ui::DataSource>,
    root: RootPlacement,
    viewport: Option<HudViewport>,
    camera: Option<glam::Mat4>,
}

impl HudSurfaces {
    /// The layer `owner` publishes.
    pub(crate) fn layer_mut(&mut self, owner: PanelUiOwner) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.owner == owner)
    }

    /// The last laid-out root size in GUI units, which embedded controls lay out against.
    pub(crate) fn root_size(&self) -> [f64; 2] {
        self.root.size
    }

    /// Places the HUD root over `frame`'s safe-area content in GUI units, as a HUD frame
    /// lays it out.
    pub(crate) fn place(&mut self, frame: &PaintFrame<'_>) {
        let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let size = frame.content.map(|axis| f64::from(axis / px));
        let origin = [frame.safe_area.left(), frame.safe_area.top()].map(|v| f64::from(v / px));
        let window = [
            size[0] + origin[0] + f64::from(frame.safe_area.right() / px),
            size[1] + origin[1] + f64::from(frame.safe_area.bottom() / px),
        ];
        self.root = RootPlacement {
            origin,
            size,
            window,
        };
        self.viewport = Some(HudViewport {
            width: size[0] as f32,
            height: size[1] as f32,
            scale: frame.metrics.gui_scale,
        })
        .filter(|viewport| viewport.valid());
    }
}

impl ModUi {
    /// Replaces the set of HUD layers, in load order. Unchanged revisions keep everything;
    /// binding-only changes repaint only the controls that read them.
    pub fn set_hud_surfaces<'a>(
        &mut self,
        surfaces: impl Iterator<Item = PublishedHudSurface<'a>>,
    ) {
        let state = &mut self.hud_surfaces;
        let root = state.root.size;
        let layers = &mut state.layers;
        let mut index = 0;
        for published in surfaces {
            match layers[index..]
                .iter()
                .position(|layer| layer.owner == published.owner)
            {
                Some(offset) => {
                    layers[index..=index + offset].rotate_right(1);
                    let layer = &mut layers[index];
                    if layer.revision != published.revision
                        && !layer.publish(
                            published.revision,
                            Arc::clone(published.surface),
                            published.authority,
                        )
                    {
                        *layer = Layer::new(
                            published.owner,
                            published.revision,
                            Arc::clone(published.surface),
                            published.authority,
                            root,
                        );
                    }
                }
                None => layers.insert(
                    index,
                    Layer::new(
                        published.owner,
                        published.revision,
                        Arc::clone(published.surface),
                        published.authority,
                        root,
                    ),
                ),
            }
            index += 1;
        }
        layers.truncate(index);
    }

    /// Actors the layers' projections follow this frame.
    pub fn hud_projected_actors(&self) -> impl Iterator<Item = u64> + '_ {
        self.hud_surfaces.layers.iter().flat_map(|layer| {
            layer
                .surface
                .bindings
                .values()
                .filter_map(|value| value.active_projection()?.actor())
        })
    }

    /// Projects every layer's world and actor points through the current camera; `actor`
    /// returns an actor's interpolated feet. Without a camera every projection hides.
    pub fn set_hud_projection(
        &mut self,
        clip_from_world: Option<[f32; 16]>,
        actor: &dyn Fn(u64) -> Option<ProjectedActor>,
    ) {
        let surfaces = &mut self.hud_surfaces;
        surfaces.camera = clip_from_world
            .filter(|matrix| matrix.iter().all(|value| value.is_finite()))
            .map(|matrix| glam::Mat4::from_cols_array(&matrix));
        let camera = surfaces.camera;
        let root = surfaces.root;
        for layer in &mut surfaces.layers {
            layer.advance(f64::NAN, false, root.size);
            layer.project(root, camera.as_ref(), actor);
        }
        if let Some(panel) = self.mod_panel.as_mut() {
            panel.project(root, camera.as_ref(), actor, self.panel_actor_authority);
        }
    }

    /// Which actors the selected panel's projections may follow.
    pub fn set_mod_panel_actor_authority(&mut self, authority: ActorAuthority) {
        self.panel_actor_authority = authority;
    }

    /// The HUD root of the last painted frame, in GUI units, and the GUI scale.
    pub fn hud_viewport(&self) -> Option<HudViewport> {
        self.hud_surfaces.viewport
    }

    /// Whether any mod has a HUD surface to draw.
    pub fn has_hud_surfaces(&self) -> bool {
        !self.hud_surfaces.layers.is_empty()
    }

    /// Paints every layer over gameplay. The caller shows them only while the gameplay HUD
    /// does; `icon` resolves item values.
    pub fn paint_hud_surfaces(
        &mut self,
        engine: Option<&FormEngine>,
        frame: &mut PaintFrame<'_>,
        icon: &dyn Fn(&str, u32) -> Option<IconRef>,
    ) {
        let surfaces = &mut self.hud_surfaces;
        surfaces.place(frame);
        let Some(engine) = engine else {
            return;
        };
        if surfaces.layers.is_empty() {
            return;
        }
        let size = surfaces.root.size;
        let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let now = frame.now;
        let animate = !self.reduced_motion;
        for layer in &mut surfaces.layers {
            layer.advance(now, animate, size);
            for (index, part) in layer.parts.iter_mut().enumerate() {
                if part.spec.floating.is_some() && layer.moves[index].is_none() {
                    continue;
                }
                part.write_identity(&self.local_player_identity);
                if let Err(error) = part.prepare(engine, frame, size, icon, false) {
                    tracing::warn!(%error, "HUD surface control preparation failed");
                }
            }
        }
        let catalog = match &surfaces.catalog {
            Some(catalog) => Arc::clone(catalog),
            None => match replay_catalog() {
                Ok(catalog) => Arc::clone(surfaces.catalog.insert(Arc::new(catalog))),
                Err(error) => {
                    tracing::warn!(%error, "HUD surface replay screen rejected");
                    return;
                }
            },
        };
        let language = frame.language;
        let data = Arc::clone(&surfaces.data);
        let mut screen = std::mem::take(&mut surfaces.screen);
        let mut replay = Replay {
            layers: &mut surfaces.layers,
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut replay];
        let result = engine.draw(
            ScreenArt::default(),
            &mut frame.with_renderers(&mut renderers),
            |env, root| {
                screen.render_shared_with(
                    SCREEN,
                    &catalog,
                    crate::personal_context(),
                    data,
                    (root, px, language),
                    env,
                    &ViewState::default(),
                )
            },
        );
        surfaces.screen = screen;
        if let Err(error) = result {
            tracing::warn!(%error, "HUD surface paint failed");
        }
    }
}

/// The static screen whose one custom control replays every layer.
fn replay_catalog() -> Result<Catalog, String> {
    Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        (
            "ui/_ui_defs.json",
            &br#"{"ui_defs":["ui/cinnabar_hud_surfaces.json"]}"#[..],
        ),
        ("ui/cinnabar_hud_surfaces.json", TEMPLATE),
    ])
    .map_err(|error| error.to_string())
}

/// Replays every prepared part at its painted position, or moved by its projection.
struct Replay<'a> {
    layers: &'a mut [Layer],
}

impl CustomRenderer for Replay<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != RENDERER {
            return Ok(false);
        }
        let px = canvas.px();
        let opacity = f32::from(node.alpha([255; 4])[3]) / 255.;
        for layer in self.layers.iter_mut() {
            let center = layer.root().map(|axis| axis * 0.5);
            for (index, part) in layer.parts.iter().enumerate() {
                let mut origin = [node.dest[0], node.dest[1]];
                if part.spec.floating.is_some() {
                    let Some(at) = layer.moves[index] else {
                        continue;
                    };
                    origin[0] += ((at[0] - center[0]) as f32) * px;
                    origin[1] += ((at[1] - center[1]) as f32) * px;
                }
                part.replay(
                    &mut layer.replays[index],
                    canvas,
                    origin,
                    1.,
                    node.dest,
                    opacity,
                )?;
            }
        }
        Ok(true)
    }

    /// Keeps every drawn part's textures resident before its artwork replays.
    fn textures<'s>(&'s self, _: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        for layer in self.layers.iter() {
            for part in &layer.parts {
                for path in part.textures.paths() {
                    texture(path);
                }
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    /// Each layer's owner and its bindings as its controls read them: item values as icon
    /// indices, projections as GUI offsets with their companions. Host probes read this.
    pub fn hud_surface_bindings(
        &self,
        owner: PanelUiOwner,
    ) -> Option<std::collections::BTreeMap<String, json_ui::Scalar>> {
        let layer = self.hud_surfaces.layers.iter().find(|l| l.owner == owner)?;
        Some(layer.resolved())
    }

    /// Paints each embedded control of `owner`'s layer has run, by control name; none for a
    /// control that cannot draw.
    pub fn hud_surface_embed_paints(
        &self,
        owner: PanelUiOwner,
    ) -> std::collections::BTreeMap<String, Option<usize>> {
        self.hud_surfaces
            .layers
            .iter()
            .find(|layer| layer.owner == owner)
            .map(|layer| {
                layer
                    .embedded
                    .iter()
                    .map(|(name, embedded)| {
                        (name.clone(), embedded.part.as_ref().map(|part| part.paints))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Paints each layer's parts have run, in paint order.
    pub fn hud_surface_paints(&self, owner: PanelUiOwner) -> Vec<usize> {
        self.hud_surfaces
            .layers
            .iter()
            .find(|layer| layer.owner == owner)
            .map_or_else(Vec::new, |layer| {
                layer.parts.iter().map(|p| p.paints).collect()
            })
    }
}
