//! One independently cached control: laid out and painted only when its inputs change, then
//! replayed from shared nodes.

use std::sync::Arc;

use json_ui::{Catalog, DataSource, DrawNode, Scalar, ViewState};
use ui::{IconRef, UiNode, mod_panel::SurfaceItem};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        CachedFragment, CachedFragmentTextures, FormEngine, ScreenArt,
        item_renderer::ItemRenderer,
        screen::{CustomRenderer, PaintError, PaintFrame},
    },
};

use super::document::PartSpec;

/// Everything a cached paint depends on beyond the part's own data.
type PaintKey = [u64; 9];

pub(crate) struct Part {
    pub spec: PartSpec,
    catalog: Arc<Catalog>,
    data: Arc<DataSource>,
    revision: u64,
    screen: CachedScreen,
    /// Bound item bindings this part reads and the item each names.
    items: Vec<(String, SurfaceItem)>,
    resolved: Vec<Option<IconRef>>,
    icons: Vec<IconRef>,
    laid: Option<Arc<[DrawNode]>>,
    /// Painted artwork, translated to a zero origin.
    pub nodes: Arc<[UiNode]>,
    pub textures: CachedFragmentTextures,
    /// Painted bounds in canvas logical pixels, before translation.
    pub bounds: Option<ui::UiRect>,
    /// The laid-out root control `[x, y, width, height]` in GUI units, when measured.
    pub rect: Option<[f64; 4]>,
    /// Logical pixels per GUI unit the artwork was painted at.
    pub px: f32,
    key: Option<PaintKey>,
    language: [usize; 3],
    canvas: [f64; 2],
    prepared: Option<u64>,
    /// Whether the part reads host identity bindings.
    identity: bool,
    /// Paints run, for cache tests and measurements.
    pub paints: usize,
}

impl Part {
    /// A part reading `data`; nothing is laid out until it is prepared.
    pub fn new(catalog: Arc<Catalog>, spec: PartSpec, data: DataSource) -> Self {
        use ui::mod_panel::local_player_head::{AVAILABLE, HEAD, NAME, UNAVAILABLE};
        let identity = [NAME, HEAD, AVAILABLE, UNAVAILABLE]
            .iter()
            .any(|name| spec.bindings.contains(*name));
        Self {
            identity,
            spec,
            catalog,
            data: Arc::new(data),
            revision: 0,
            screen: CachedScreen::default(),
            items: Vec::new(),
            resolved: Vec::new(),
            icons: Vec::new(),
            laid: None,
            nodes: Arc::from([]),
            textures: CachedFragmentTextures::default(),
            bounds: None,
            rect: None,
            px: 1.,
            key: None,
            language: [0; 3],
            canvas: [0.; 2],
            prepared: None,
            paints: 0,
        }
    }

    /// The data the part lays out with.
    #[cfg(any(test, feature = "test-support"))]
    pub fn data(&self) -> &DataSource {
        &self.data
    }

    /// Changes one global when its value differs, so the next prepare repaints.
    pub fn set_global(&mut self, name: &str, value: Scalar) {
        if self.data.global_value(name) != Some(&value) {
            Arc::make_mut(&mut self.data).set_global(name, value);
            self.revision += 1;
        }
    }

    /// Replaces a collection's items and defaults.
    pub fn set_collection(
        &mut self,
        name: &str,
        items: Vec<json_ui::CollectionItem>,
        dimensions: Scalar,
    ) {
        let data = Arc::make_mut(&mut self.data);
        data.set_collection(name, items);
        data.set_collection_defaults(name, [("#item_exists".into(), Scalar::Bool(false))].into());
        data.set_global(format!("#collection_{name}_dimensions"), dimensions);
        self.revision += 1;
    }

    /// Removes one global, so the part reads it as unbound, as a part built without it does.
    /// Retained bindings keep the last value a global answered, so the next prepare binds
    /// afresh.
    pub fn unset_global(&mut self, name: &str) {
        if self.data.global_value(name).is_some() {
            Arc::make_mut(&mut self.data).remove_global(name);
            self.screen = CachedScreen::default();
            self.revision += 1;
        }
    }

    /// Names the item an item binding draws; none stops resolving the binding, leaving its
    /// global to the caller.
    pub fn set_item(&mut self, name: &str, item: Option<SurfaceItem>) {
        let position = self.items.iter().position(|(binding, _)| binding == name);
        match (position, item) {
            (Some(position), Some(item)) if self.items[position].1 == item => return,
            (Some(position), Some(item)) => self.items[position].1 = item,
            (Some(position), None) => {
                self.items.remove(position);
                if self.items.is_empty() {
                    self.icons.clear();
                }
            }
            (None, Some(item)) => self.items.push((name.to_owned(), item)),
            (None, None) => return,
        }
        self.resolved.clear();
    }

    /// Applies host identity bindings when this part reads them.
    pub fn write_identity(&mut self, identity: &crate::local_player_identity::LocalPlayerIdentity) {
        if !self.identity {
            return;
        }
        let before = Arc::as_ptr(&self.data);
        identity.write(&mut self.data);
        if Arc::as_ptr(&self.data) != before {
            self.revision += 1;
        }
    }

    /// Lays out and paints the part at `canvas` GUI units when any input changed, keeping the
    /// shared nodes of an unchanged paint. With `measure`, also records the root's laid-out
    /// rectangle.
    pub fn prepare(
        &mut self,
        engine: &FormEngine,
        frame: &mut PaintFrame<'_>,
        canvas: [f64; 2],
        icon: &dyn Fn(&str, u32) -> Option<IconRef>,
        measure: bool,
    ) -> Result<(), PaintError> {
        let mut changed = self.resolve_icons(icon);
        let mut changed_textures = self
            .laid
            .as_ref()
            .is_some_and(|laid| engine.refresh_fragment_textures(&mut self.textures, laid));
        let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let key = [
            frame.font as *const _ as usize as u64,
            u64::from(px.to_bits()),
            u64::from(frame.metrics.gui_scale.to_bits()),
            u64::from(frame.metrics.dpi_scale.get().to_bits()),
            u64::from(frame.metrics.line_height_64),
            u64::from(frame.metrics.baseline_64),
            u64::from(frame.solid_page),
            engine as *const _ as usize as u64,
            engine.textures.metadata_revision(),
        ];
        changed |= self.canvas != canvas || (measure && self.rect.is_none());
        if !changed
            && !changed_textures
            && self.key == Some(key)
            && self.language == frame.language
            && self.prepared == Some(self.revision)
        {
            return Ok(());
        }
        if self.key != Some(key) {
            self.screen.invalidate_textures();
        }
        let mut items = ItemRenderer {
            icons: &self.icons,
            id_aux: &[],
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut items];
        let mut target = frame.with_renderers(&mut renderers);
        let language = target.language;
        let mut rect = None;
        let laid = engine
            .prepare(ScreenArt::default(), &mut target, |env, _| {
                let render = self.screen.render_shared_with(
                    &self.spec.reference,
                    &self.catalog,
                    crate::personal_context(),
                    Arc::clone(&self.data),
                    (canvas, px, language),
                    env,
                    &ViewState::default(),
                )?;
                if measure {
                    let root = json_ui::layout(&render.bound, canvas, env).rect;
                    rect = Some([root.x, root.y, root.w, root.h]);
                }
                Some(render)
            })
            .map(|render| Arc::clone(&render.nodes));
        let Some(laid) = laid else {
            self.nodes = Arc::from([]);
            self.bounds = None;
            return Ok(());
        };
        if measure {
            self.rect = rect;
        }
        changed_textures |= engine.refresh_fragment_textures(&mut self.textures, &laid);
        if !changed
            && !changed_textures
            && self.key == Some(key)
            && self
                .laid
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, &laid))
        {
            self.prepared = Some(self.revision);
            self.language = language;
            return Ok(());
        }
        let mut nodes = Vec::new();
        let mut next = 0;
        let content = canvas.map(|axis| axis as f32 * px);
        let mut output = PaintFrame {
            nodes: &mut nodes,
            next: &mut next,
            layouts: &mut *target.layouts,
            font: target.font,
            metrics: target.metrics,
            solid_page: target.solid_page,
            safe_area: ui::SafeArea::ZERO,
            content,
            translate: target.translate,
            language,
            now: target.now,
            renderers: target.renderers,
        };
        engine.draw(ScreenArt::default(), &mut output, |env, _| {
            self.screen.render_shared_with(
                &self.spec.reference,
                &self.catalog,
                crate::personal_context(),
                Arc::clone(&self.data),
                (canvas, px, language),
                env,
                &ViewState::default(),
            )
        })?;
        self.bounds =
            ui_engine::engine::normalize_fragment(&mut nodes, [0., 0., content[0], content[1]])?;
        self.nodes = nodes.into();
        self.px = px;
        self.laid = Some(laid);
        self.key = Some(key);
        self.canvas = canvas;
        self.prepared = Some(self.revision);
        self.language = language;
        self.paints += 1;
        Ok(())
    }

    /// Resolves item bindings to icon indices, reporting whether any changed.
    fn resolve_icons(&mut self, icon: &dyn Fn(&str, u32) -> Option<IconRef>) -> bool {
        let mut changed = self.resolved.len() != self.items.len();
        self.resolved.resize(self.items.len(), None);
        for ((_, item), resolved) in self.items.iter().zip(&mut self.resolved) {
            let next = icon(&item.identifier, item.metadata);
            changed |= *resolved != next;
            *resolved = next;
        }
        if changed {
            self.icons.clear();
            let data = Arc::make_mut(&mut self.data);
            for ((name, _), resolved) in self.items.iter().zip(&self.resolved) {
                let value = resolved.map_or(Scalar::Json(serde_json::Value::Null), |icon| {
                    self.icons.push(icon);
                    Scalar::Num((self.icons.len() - 1) as f64)
                });
                data.set_global(name.as_str(), value);
            }
            self.revision += 1;
        }
        changed
    }

    /// Replays the cached artwork through `cache`, one per drawn instance, with its canvas
    /// origin at `origin`, scaled by `scale` and clipped to `clip`, in canvas logical pixels.
    pub fn replay(
        &self,
        cache: &mut CachedFragment,
        canvas: &mut ui_engine::engine::Canvas<'_>,
        origin: [f32; 2],
        scale: f32,
        clip: [f32; 4],
        opacity: f32,
    ) -> Result<(), PaintError> {
        if !self.textures.matches(canvas.textures()) {
            return Ok(());
        }
        let Some(bounds) = self.bounds else {
            return Ok(());
        };
        let ratio = canvas.px() / self.px * scale;
        let source = [bounds.width(), bounds.height()];
        let at = [
            origin[0] + bounds.min().x() * ratio,
            origin[1] + bounds.min().y() * ratio,
        ];
        let dest = [
            at[0],
            at[1],
            at[0] + source[0] * ratio,
            at[1] + source[1] * ratio,
        ];
        canvas.paint_cached_fragment(cache, &self.nodes, source, dest, clip, opacity)
    }
}
