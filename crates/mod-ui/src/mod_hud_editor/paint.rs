//! Painting for the host-owned touch layout editor.
use super::*;
impl HudEditor {
    /// Paints the current draft and its editing controls without extension identity or controls.
    pub fn paint(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        animate: bool,
    ) {
        let Some(renderer) = engine else {
            self.finish(false);
            return;
        };
        let editor = self;
        if !editor.painting() {
            return;
        }
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let content = paint_frame.content;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if viewport[0] < 220. || viewport[1] < 120. {
            editor.finish(false);

            return;
        }
        if editor.viewport != viewport {
            editor.data_dirty = true;
            editor.viewport = viewport;
            editor.catalog = None;
            editor.frame = None;
            editor.cancel_pointer_input();
            editor.anchor_motion = None;
        }
        editor.advance_transitions(paint_frame.now, animate);
        editor.advance_feedback(paint_frame.now, animate);
        if !editor.painting() {
            return;
        }
        if editor.data_dirty {
            let mut data = editor.data();
            if editor.draft.surface.is_some() {
                editor.transitions.write(&mut data);
            }
            if *editor.data != data {
                editor.data = Arc::new(data);
            }
            editor.data_dirty = false;
        }
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let world = editor
            .draft
            .surface
            .as_ref()
            .map(|surface| editor.transitions.world_backdrop(surface))
            .unwrap_or_default();
        if let Err(error) = paint_world_backdrop(paint_frame, world) {
            paint_frame.nodes.truncate(rollback.0);
            *paint_frame.next = rollback.1;
            editor.finish(false);
            tracing::warn!(%error, "HUD editor disabled after rendering failure");

            return;
        }
        let keep_out = std::mem::take(&mut editor.keep_out);
        let mut glass = GlassRenderer::new(world);
        let mut zones = feedback::KeepOutRenderer { rects: &keep_out };
        let mut renderers: [&mut dyn CustomRenderer; 2] = [&mut glass, &mut zones];
        // Keep pointer feedback outside the editor borrow while measuring native hit bounds.
        let view = std::mem::take(&mut editor.view);
        let art = ScreenArt {
            view: Some(&view),
            ..Default::default()
        };
        let result = renderer.draw(
            art,
            &mut paint_frame.with_renderers(&mut renderers),
            |env, root| {
                if editor.measure_caption(env.text) {
                    let mut data = (*editor.data).clone();
                    editor.write_geometry(&mut data);
                    editor.data = Arc::new(data);
                }
                if editor.catalog.is_none() {
                    let catalog =
                        match template::catalog(&editor.draft, viewport, editor.history_buttons) {
                            Ok(catalog) => catalog,
                            Err(error) => {
                                tracing::warn!(%error,"HUD editor template rejected");
                                return None;
                            }
                        };
                    editor.catalog = Some(Arc::new(catalog));
                    editor.screen = CachedScreen::default();
                }
                editor.screen.render_shared_with(
                    SCREEN,
                    editor.catalog.as_ref().expect("installed"),
                    &Context::default(),
                    Arc::clone(&editor.data),
                    (root, px, language),
                    env,
                    &view,
                )
            },
        );
        editor.view = view;
        editor.keep_out = keep_out;
        match result {
            Ok(Some(frame)) => editor.frame = Some(frame),
            result => {
                paint_frame.nodes.truncate(rollback.0);
                *paint_frame.next = rollback.1;
                editor.finish(false);
                if let Err(error) = result {
                    tracing::warn!(%error, "HUD editor disabled after rendering failure");
                }
            }
        }
    }
}
