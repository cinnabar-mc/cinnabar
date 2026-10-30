//! A private JSON-UI catalog for bounded personal-extension controls.

mod data;
mod edit;
mod field;
mod field_paint;
mod input;
mod motion;
pub(crate) mod prepared;
mod reveal;
mod scrolling;
pub(crate) mod surface;
mod template;
pub(crate) mod transitions;
mod vector;
mod widgets;

#[cfg(any(test, feature = "test-support"))]
pub(crate) use template::same_shape;

#[cfg(test)]
use json_ui::Context;
use std::sync::Arc;

use json_ui::{Catalog, DataSource, ViewState};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, IconRef, UiScale,
    mod_panel::{Control, Event, Panel},
};

use crate::{ModUi, PanelUiOwner};
use data::control_data;
use ui_engine::frame::EngineFrame;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        glass::{GlassRenderer, paint_world_backdrop},
        screen::{CustomRenderer, PaintFrame},
    },
};
pub(crate) use widgets::Palette;

const SCREEN: &str = "cinnabar_personal.panel";

pub(super) struct ModPanel {
    pub(super) owner: PanelUiOwner,
    embeds: crate::hud_surface::embed::Embeds,
    panel: Panel,
    open: bool,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    frame: Option<EngineFrame>,
    viewport: [f64; 2],
    drag: Option<usize>,
    vector_drag: Option<vector::Drag>,
    scroll_drag: Option<(String, f64)>,
    view: ViewState,
    data: Arc<DataSource>,
    pointer: Option<[f32; 2]>,
    held: bool,
    edit: Option<edit::Editor>,
    /// Native feedback and surface transitions, including an exit after closing.
    motion: motion::PanelMotion,
    motion_data_dirty: bool,
    text_events: Vec<ui::mod_panel::TextEvent>,
    field_events: Vec<ui::mod_panel::FieldEvent>,
    prepared: Option<Arc<prepared::PreparedRoute>>,
    collection_reveals: reveal::CollectionReveals,
    /// Item values of the surface, as icon indices into `icons`.
    items: Vec<(String, Option<IconRef>)>,
    icons: Vec<IconRef>,
    /// The last projected offset of each projected binding.
    projected: std::collections::BTreeMap<String, Option<[f64; 2]>>,
    /// Where the panel was last laid out over the HUD root.
    layout: Option<ui::mod_hud::PanelLayout>,
}

impl ModUi {
    /// Drains committed native text edits once for the active personal-panel owner.
    pub fn take_mod_panel_text_events(
        &mut self,
        owner: PanelUiOwner,
    ) -> Vec<ui::mod_panel::TextEvent> {
        self.mod_panel
            .as_mut()
            .filter(|panel| panel.owner == owner)
            .map_or_else(Vec::new, |panel| std::mem::take(&mut panel.text_events))
    }
    /// Drains live edits, submissions and navigation once for the matching panel owner.
    pub fn take_mod_panel_field_events(
        &mut self,
        owner: PanelUiOwner,
    ) -> Vec<ui::mod_panel::FieldEvent> {
        self.mod_panel
            .as_mut()
            .filter(|panel| panel.owner == owner)
            .map_or_else(Vec::new, |panel| panel.drain_field_events())
    }

    /// Drops the panel's laid-out text and pointer capture after its private font changed.
    pub fn invalidate_mod_panel_font(&mut self) {
        for owner in self.prepared_panel_ui.values_mut() {
            for screen in &mut owner.screens {
                *screen = None;
            }
        }
        if let Some(panel) = &mut self.mod_panel {
            panel.cancel_edit();
            panel.screen = CachedScreen::default();
            panel.frame = None;
            panel.drag = None;
            panel.vector_drag = None;
            panel.scroll_drag = None;
            panel.pointer = None;
        }
    }

    /// Releases all panel catalogs and input when the last local instance leaves.
    pub fn clear_mod_panels(&mut self) {
        self.mod_panel = None;
        self.prepared_panel_ui.clear();
    }

    /// Publishes one owner's panel, retaining its open state across value updates.
    /// Passing no panel clears the selection only when it belongs to this owner.
    pub fn set_mod_panel(
        &mut self,
        owner: PanelUiOwner,
        panel: Option<&Panel>,
    ) -> Result<(), String> {
        let Some(panel) = panel else {
            if self
                .mod_panel
                .as_ref()
                .is_some_and(|panel| panel.owner == owner)
            {
                self.mod_panel = None;
            }
            return Ok(());
        };
        if self
            .mod_panel
            .as_ref()
            .is_some_and(|current| current.owner == owner && current.panel == *panel)
        {
            return Ok(());
        }
        let same_shape = self.mod_panel.as_ref().is_some_and(|current| {
            current.owner == owner && template::same_shape(&current.panel, panel)
        });
        let prepared = self
            .prepared_panel_ui
            .get(&owner)
            .and_then(|prepared| prepared.prepared.route(panel));
        if same_shape || prepared.is_some() {
            panel.validate_values()?;
        } else {
            panel.validate()?;
        }
        if let Some(current) = self.mod_panel.as_mut() {
            let data_changed = !same_shape || !data::same_values(&current.panel, panel);
            if current.owner != owner {
                current.embeds = Default::default();
            }
            if !same_shape {
                let transitions = (current.owner == owner && current.open)
                    .then(|| std::mem::take(&mut current.motion.transitions));
                current.text_events.clear();
                current.field_events.clear();
                current.owner = owner;
                current.edit = None;
                current.screen = CachedScreen::default();
                current.catalog = None;
                current.frame = None;
                current.drag = None;
                current.vector_drag = None;
                current.scroll_drag = None;
                current.items.clear();
                current.projected.clear();
                current.view = ViewState::default();
                current.motion = motion::PanelMotion::default();
                if let Some(transitions) = transitions {
                    current.motion.transitions = transitions;
                }
                current.pointer = None;
                current.prepared = prepared.clone();
                current.catalog = prepared.as_ref().map(|route| Arc::clone(&route.catalog));
            }
            if panel.capture_key && !current.editing_text() {
                current.cancel_edit();
            }
            current.reconcile_editor_value(panel);
            if !same_shape {
                current.collection_reveals = reveal::CollectionReveals::default();
            }
            current.collection_reveals.publish(panel);
            current.panel = panel.clone();
            if data_changed {
                current.refresh_data();
            }
        } else {
            self.mod_panel = Some(ModPanel {
                owner,
                embeds: Default::default(),
                panel: panel.clone(),
                open: false,
                catalog: prepared.as_ref().map(|route| Arc::clone(&route.catalog)),
                screen: CachedScreen::default(),
                frame: None,
                viewport: [0.0; 2],
                drag: None,
                vector_drag: None,
                scroll_drag: None,
                view: ViewState::default(),
                data: Arc::new({
                    let mut data = control_data(panel);
                    surface::viewport(&mut data, [0.0; 2]);
                    data
                }),
                pointer: None,
                held: false,
                edit: None,
                motion: motion::PanelMotion::default(),
                motion_data_dirty: false,
                text_events: Vec::new(),
                field_events: Vec::new(),
                prepared,
                collection_reveals: {
                    let mut requests = reveal::CollectionReveals::default();
                    requests.publish(panel);
                    requests
                },
                items: Vec::new(),
                icons: Vec::new(),
                projected: Default::default(),
                layout: None,
            });
        }
        if (self.reduced_motion || !panel.motion)
            && let Some(current) = self.mod_panel.as_mut()
        {
            current.snap_motion();
        }
        Ok(())
    }

    /// Changes visibility once per transition and restores animated feedback when closing.
    /// Closing releases input at once; a painted surface whose transitions declare exit values
    /// keeps drawing until they settle, including authored panel chrome.
    /// Reopening during that exit eases back from the displayed values.
    pub fn set_mod_panel_open(&mut self, open: bool) {
        let exit = !self.reduced_motion;
        if let Some(panel) = self.mod_panel.as_mut() {
            if open {
                panel.open = true;
                panel.motion.transitions.cancel_exit();
                if self.reduced_motion || !panel.panel.motion {
                    panel.snap_motion();
                }
            } else {
                panel.close(exit);
            }
        }
    }

    /// Whether the panel is open and owns input; false while a closed panel plays its exit.
    pub fn mod_panel_open(&self) -> bool {
        self.mod_panel.as_ref().is_some_and(|panel| panel.open)
    }

    /// The open panel's owner and where it was last laid out over the HUD root, for
    /// `panel.read-layout`; none while no panel is open.
    pub fn mod_panel_layout(&self) -> Option<(PanelUiOwner, ui::mod_hud::PanelLayout)> {
        let panel = self.mod_panel.as_ref().filter(|panel| panel.open)?;
        Some((panel.owner, panel.layout?))
    }

    /// Whether the active panel retains resource-pack HUD content for placement or inspection.
    pub fn mod_panel_shows_gameplay_hud(&self) -> bool {
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open && panel.panel.show_gameplay_hud)
    }

    /// Whether focused keys are owned by guest behavior instead of the native close shortcut.
    pub fn mod_panel_capture_key(&self) -> bool {
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.panel.capture_key)
    }

    /// Whether the panel draws: open, or closed while bindings ease to their exit values.
    pub fn mod_panel_painting(&self) -> bool {
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open || panel.exiting())
    }

    /// Native feedback, surface transitions and exits need frames only while they are moving.
    pub fn mod_panel_motion_active(&self) -> bool {
        self.mod_panel.as_ref().is_some_and(|panel| {
            panel.exiting()
                || panel.open
                    && (panel.motion.active()
                        || panel.motion_data_dirty && panel.animated()
                        || matches!(panel.edit, Some(edit::Editor::Field { .. })))
        })
    }

    /// Explicitly overrides mod surface motion, independently of built-in screen preferences.
    /// Disabling settles transitions, exits and opted-in feedback, including values already moving.
    pub fn configure_motion(&mut self, enabled: bool) {
        let reduced_motion = !enabled;
        if self.reduced_motion == reduced_motion {
            return;
        }
        self.reduced_motion = reduced_motion;
        if !enabled && let Some(panel) = self.mod_panel.as_mut() {
            panel.snap_motion();
        }
    }

    pub fn mod_panel_toggle_key(&self) -> Option<&str> {
        self.mod_panel
            .as_ref()
            .map(|panel| panel.panel.toggle_key.as_str())
    }

    /// Events use only geometry from the last rendered frame. Slider drags stay captured.
    pub fn mod_panel_events(
        &mut self,
        position: [f32; 2],
        pressed: bool,
        held: bool,
    ) -> Vec<Event> {
        let exit = !self.reduced_motion;
        self.mod_panel.as_mut().map_or_else(Vec::new, |panel| {
            panel.pointer_events(position, pressed, held, exit)
        })
    }

    /// Draws the open or exiting panel, fitted to its authored reference size.
    pub fn paint_mod_panel(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
    ) {
        self.paint_mod_panel_with_icons(engine, paint_frame, &|_, _| None);
    }

    /// Paints live previews using the caller's current item atlas, resolving only requested rows.
    pub fn paint_mod_panel_with_icons(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        icon: &dyn Fn(&str, u32) -> Option<ui::IconRef>,
    ) {
        let hud_metrics = paint_frame.metrics;
        let animate = !self.reduced_motion;
        let Some(panel) = self
            .mod_panel
            .as_mut()
            .filter(|panel| panel.open || panel.exiting())
        else {
            return;
        };
        let Some(renderer) = engine else {
            panel.close(false);
            return;
        };
        let content = paint_frame.content;
        // Authored surfaces may declare their logical extent for viewport fitting.
        let scale = panel.panel.reference_size.map_or(1.0, |size| {
            content
                .into_iter()
                .zip(size)
                .map(|(available, required)| {
                    available / (required * FONT_DESIGN_PIXEL_TEXELS as f32)
                })
                .fold(1.0, f32::min)
                .clamp(UiScale::DISPLAY_MIN, 1.0)
        });
        paint_frame.metrics.scale =
            UiScale::new_display(scale).expect("fitted display scale is valid");
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if !viewport.iter().all(|axis| axis.is_finite() && *axis > 0.) {
            panel.close(false);
            return;
        }
        // The panel root and the HUD root both lay out from the safe content's top left.
        panel.layout = Some(ui::mod_hud::PanelLayout {
            size: viewport.map(|axis| axis as f32),
            scale: px / (hud_metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32),
            offset: [0.; 2],
        })
        .filter(|layout| layout.valid());
        if panel.viewport != viewport {
            panel.cancel_edit();
            panel.viewport = viewport;
            panel.refresh_data();
            panel.frame = None;
            panel.drag = None;
            panel.vector_drag = None;
            panel.scroll_drag = None;
            panel.pointer = None;
        }
        if panel.catalog.is_none() && panel.edit.is_none() {
            panel.catalog = panel
                .prepared
                .as_ref()
                .map(|route| Arc::clone(&route.catalog));
        }
        if panel.catalog.is_none() {
            match template::catalog_prepared(
                &panel.panel,
                viewport,
                panel.edit.as_ref(),
                panel.prepared.as_ref().map(|route| route.catalog.as_ref()),
            ) {
                Ok(catalog) => {
                    panel.catalog = Some(Arc::new(catalog));
                    panel.screen = CachedScreen::default();
                }
                Err(error) => {
                    panel.close(false);
                    tracing::warn!(%error, "personal extension panel rejected");
                    return;
                }
            }
        }
        let changed = panel.advance_motion(paint_frame.now, animate);
        if !panel.open && !panel.motion.transitions.active() {
            // The exit has settled: remove the surface without drawing its final values.
            panel.close(false);
            return;
        }
        if panel.animated() && (changed || panel.motion_data_dirty) {
            panel.motion.write(Arc::make_mut(&mut panel.data));
            panel.motion_data_dirty = false;
        }
        self.local_player_identity.write(&mut panel.data);
        panel.resolve_items(icon);
        let data = Arc::clone(&panel.data);
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let world = panel
            .motion
            .transitions
            .world_backdrop(&panel.panel.surface);
        if let Err(error) = paint_world_backdrop(paint_frame, world) {
            out_of_render(panel, paint_frame, rollback);
            tracing::warn!(%error, "personal extension panel disabled after rendering failure");
            return;
        }
        let caller = std::mem::take(&mut paint_frame.renderers);
        let result = {
            let mut glass = GlassRenderer::new(world);
            let editing = panel.edit.is_some();
            let mut field = field_paint::FieldRenderer::new(
                panel.edit.as_mut(),
                paint_frame.now,
                Palette::for_panel(&panel.panel),
                [paint_frame.safe_area.left(), paint_frame.safe_area.top()],
            );
            let mut items = ui_engine::engine::item_renderer::ItemRenderer {
                icons: &panel.icons,
                id_aux: &[],
            };
            let mut field_slot: [&mut dyn CustomRenderer; 2] = [&mut field, &mut items];
            let mut host = RendererChain {
                first: &mut glass,
                rest: &mut field_slot,
            };
            let mut base = RendererChain {
                first: &mut host,
                rest: &mut *caller,
            };
            let language = paint_frame.language;
            let catalog = panel.catalog.as_ref().expect("catalog installed above");
            let screen = panel
                .prepared
                .as_ref()
                .filter(|_| !editing)
                .and_then(|route| {
                    self.prepared_panel_ui
                        .get_mut(&panel.owner)
                        .map(|owner| owner.screen(route))
                })
                .unwrap_or(&mut panel.screen);
            for name in panel.panel.surface.collections.keys() {
                screen.virtualize_clipped_grid(
                    name,
                    ui::mod_panel::SurfaceCollection::INITIAL_WINDOW,
                );
            }
            let laid = {
                let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut base];
                renderer
                    .prepare(
                        ScreenArt::default(),
                        &mut paint_frame.with_renderers(&mut renderers),
                        |env, root| {
                            panel.collection_reveals.apply(
                                &panel.panel,
                                catalog,
                                &mut panel.view,
                                &mut panel.motion,
                                env,
                            );
                            screen.render_shared_with(
                                SCREEN,
                                catalog,
                                crate::personal_context(),
                                Arc::clone(&data),
                                (root, px, language),
                                env,
                                &panel.view,
                            )
                        },
                    )
                    .map(|render| Arc::clone(&render.nodes))
            };
            if let Some(laid) = laid {
                let panel_metrics = paint_frame.metrics;
                paint_frame.metrics = hud_metrics;
                panel.embeds.prepare(
                    &laid,
                    &panel.view,
                    panel.owner,
                    &mut self.hud_surfaces,
                    &self.local_player_identity,
                    renderer,
                    paint_frame,
                    icon,
                );
                paint_frame.metrics = panel_metrics;
            }
            let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut base];
            let mut previews = panel.embeds.renderer(panel.owner, &mut self.hud_surfaces);
            let mut combined = RendererChain {
                first: &mut previews,
                rest: &mut renderers,
            };
            let mut preview_renderers: [&mut dyn CustomRenderer; 1] = [&mut combined];
            let art = ScreenArt {
                view: Some(&panel.view),
                ..Default::default()
            };
            renderer.draw(
                art,
                &mut paint_frame.with_renderers(&mut preview_renderers),
                |env, root| {
                    screen.render_shared_with(
                        SCREEN,
                        catalog,
                        crate::personal_context(),
                        data,
                        (root, px, language),
                        env,
                        &panel.view,
                    )
                },
            )
        };
        paint_frame.renderers = caller;
        match result {
            Ok(Some(mut frame)) => {
                if !panel.panel.surface.collections.is_empty() {
                    surface::resolve_collection_hits(
                        &panel.panel.surface,
                        Arc::make_mut(&mut frame.hits),
                    );
                }
                for (key, metrics) in &frame.report.scrolls {
                    if let Some(offset) = panel.view.scroll.get_mut(key) {
                        *offset = metrics.offset;
                    }
                }
                panel.frame = Some(frame);
            }
            result => {
                out_of_render(panel, paint_frame, rollback);
                if let Err(error) = result {
                    tracing::warn!(%error, "personal extension panel disabled after rendering failure");
                }
            }
        }
    }
}

/// Borrows a host renderer and the caller's renderers without allocating a dispatch list.
struct RendererChain<'a, 'b> {
    first: &'a mut dyn CustomRenderer,
    rest: &'a mut [&'b mut dyn CustomRenderer],
}

impl CustomRenderer for RendererChain<'_, '_> {
    /// Tries the host renderer first, then the caller's borrowed renderers.
    fn paint(
        &mut self,
        node: &ui_engine::engine::screen::CustomDraw<'_>,
        canvas: &mut ui_engine::engine::Canvas<'_>,
    ) -> Result<bool, ui_engine::engine::screen::PaintError> {
        if self.first.paint(node, canvas)? {
            return Ok(true);
        }
        for renderer in self.rest.iter_mut() {
            if renderer.paint(node, canvas)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Preserves every renderer's texture residency declarations.
    fn textures<'s>(&'s self, drawn: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        self.first.textures(drawn, texture);
        for renderer in self.rest.iter() {
            renderer.textures(drawn, texture);
        }
    }
}

fn out_of_render(panel: &mut ModPanel, paint_frame: &mut PaintFrame<'_>, rollback: (usize, u32)) {
    paint_frame.nodes.truncate(rollback.0);
    *paint_frame.next = rollback.1;
    panel.close(false);
}

impl ModPanel {
    /// Samples native feedback and surface transitions under both animation preferences.
    fn advance_motion(&mut self, now: f64, animate: bool) -> bool {
        let animate = animate && self.panel.motion;
        let mut changed = false;
        if self.panel.motion {
            if let Some(frame) = &self.frame {
                for (key, metrics) in &frame.report.scrolls {
                    self.motion.scroll_extent(key, metrics.max_offset());
                }
            }
            let hovered = self
                .frame
                .as_ref()
                .and_then(|frame| {
                    frame
                        .hits
                        .iter()
                        .find(|hit| self.view.hovered.as_deref() == Some(hit.key.as_str()))
                })
                .and_then(|hit| hit.pressed.as_deref())
                .and_then(|action| {
                    action
                        .strip_prefix("mod.control:")
                        .or_else(|| action.strip_prefix("mod.edit:"))
                })
                .and_then(|index| index.parse().ok());
            changed = self
                .motion
                .advance(&self.panel, &mut self.view, hovered, now, animate);
        }
        changed |= self
            .motion
            .transitions
            .advance(&self.panel.surface, now, animate);
        changed
    }

    /// Completes pending feedback and surface motion without restarting the current entrance.
    /// A closed panel's pending exit is removed immediately instead of being retained for paint.
    fn snap_motion(&mut self) {
        if !self.open {
            self.close(false);
            return;
        }
        let mut changed =
            self.motion
                .advance(&self.panel, &mut self.view, None, self.motion.now, false);
        changed |= self
            .motion
            .transitions
            .advance(&self.panel.surface, self.motion.now, false);
        if changed || self.motion_data_dirty {
            self.motion.write(Arc::make_mut(&mut self.data));
        }
        self.motion_data_dirty = false;
    }

    /// Closes an open panel once, releasing input at once. With `exit`, a painted surface whose
    /// transitions declare exit values keeps its frame and scroll while they ease there; the
    /// panel is otherwise removed at once, clearing scroll state and animated feedback. Without
    /// `exit`, this also ends an exit in progress, which a repeated animated close continues.
    fn close(&mut self, exit: bool) {
        let exit = exit && self.panel.motion;
        if !self.open && (exit || !self.exiting()) {
            return;
        }
        let reset_feedback = self.motion.needs_feedback_reset() || self.exiting();
        if self.open {
            self.open = false;
            self.cancel_edit();
            self.cancel_pointer_input();
            if exit && self.motion.transitions.begin_exit(&self.panel.surface) {
                self.view.focused = None;
                self.motion_data_dirty = true;
                return;
            }
        }
        self.frame = None;
        self.view = ViewState::default();
        self.motion = motion::PanelMotion::default();
        self.collection_reveals = reveal::CollectionReveals::default();
        if reset_feedback {
            self.refresh_data();
        }
    }

    /// Whether the closed panel still draws while bindings ease to their exit values.
    fn exiting(&self) -> bool {
        !self.open && self.motion.transitions.exiting()
    }

    /// Synchronizes disabled feedback after local edits, then publishes values and viewport size.
    fn refresh_data(&mut self) {
        if !self.panel.motion {
            self.motion
                .advance(&self.panel, &mut self.view, None, self.motion.now, false);
        }
        let mut data = control_data(&self.panel);
        surface::viewport(&mut data, self.viewport);
        self.data = Arc::new(data);
        self.items.clear();
        self.projected.clear();
        self.motion_data_dirty = true;
    }

    /// Projects the surface's projected bindings onto the panel root, which covers the HUD
    /// root `hud` at the panel's own scale, writing changed offsets and their companions.
    pub(crate) fn project(
        &mut self,
        hud: crate::hud_surface::RootPlacement,
        camera: Option<&glam::Mat4>,
        actor: &dyn Fn(u64) -> Option<crate::hud_surface::ProjectedActor>,
        authority: crate::hud_surface::ActorAuthority,
    ) {
        if !hud.size[0].is_finite() || hud.size[0] <= 0. || self.viewport[0] <= 0. {
            return;
        }
        let scale = self.viewport[0] / hud.size[0];
        let root = crate::hud_surface::RootPlacement {
            origin: hud.origin.map(|axis| axis * scale),
            size: self.viewport,
            window: hud.window.map(|axis| axis * scale),
        };
        for (name, value) in &self.panel.surface.bindings {
            if value.projection().is_none() {
                continue;
            }
            let offset = value.active_projection().and_then(|projection| {
                crate::hud_surface::project(projection, camera, root, actor, authority)
            });
            if self.projected.get(name) == Some(&offset) {
                continue;
            }
            self.projected.insert(name.clone(), offset);
            let data = Arc::make_mut(&mut self.data);
            data.set_global(
                name.as_str(),
                json_ui::Scalar::Json(serde_json::json!(offset.unwrap_or([0.; 2]))),
            );
            data.set_global(
                ui::mod_panel::projected_visible_name(name),
                json_ui::Scalar::Bool(offset.is_some()),
            );
        }
    }

    /// Resolves the surface's item values to icon indices, rewriting only changed bindings.
    fn resolve_items(&mut self, icon: &dyn Fn(&str, u32) -> Option<IconRef>) {
        let mut changed = false;
        let mut index = 0;
        for (name, value) in &self.panel.surface.bindings {
            let Some(item) = value.item() else {
                continue;
            };
            let resolved = icon(&item.identifier, item.metadata);
            match self.items.get(index) {
                Some((current, previous)) if current == name && *previous == resolved => {}
                _ => {
                    changed = true;
                    self.items.truncate(index);
                    self.items.push((name.clone(), resolved));
                }
            }
            index += 1;
        }
        changed |= self.items.len() != index;
        self.items.truncate(index);
        if !changed {
            return;
        }
        self.icons.clear();
        let data = Arc::make_mut(&mut self.data);
        for (name, resolved) in &self.items {
            let value = resolved.map_or(json_ui::Scalar::Json(serde_json::Value::Null), |icon| {
                self.icons.push(icon);
                json_ui::Scalar::Num((self.icons.len() - 1) as f64)
            });
            data.set_global(name.as_str(), value);
        }
    }

    /// Whether native feedback or declared surface transitions rewrite published bindings.
    fn animated(&self) -> bool {
        self.panel.motion || !self.panel.surface.transitions.is_empty()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    pub fn mod_panel_state(&self) -> Option<crate::test_support::ModPanelState<'_>> {
        let panel = self.mod_panel.as_ref()?;
        let screen = panel
            .prepared
            .as_ref()
            .filter(|_| panel.edit.is_none())
            .and_then(|route| {
                self.prepared_panel_ui
                    .get(&panel.owner)?
                    .cached_screen(route)
            })
            .unwrap_or(&panel.screen);
        Some(crate::test_support::ModPanelState {
            catalog: panel.catalog.as_ref(),
            frame: panel.frame.as_ref(),
            passes: screen.passes,
            #[cfg(feature = "test-support")]
            resolves: screen.resolves,
        })
    }

    /// Ink fits the panel's embedded HUD controls have computed.
    #[cfg(test)]
    pub(crate) fn mod_panel_embed_fits(&self) -> usize {
        self.mod_panel
            .as_ref()
            .map_or(0, |panel| panel.embeds.fit_computations())
    }
}

#[cfg(test)]
mod collection_tests;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod reveal_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;
