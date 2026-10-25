//! Records retained terrain draws and exposes their triangle streams for geometry expansion.

use super::ShaderpackGeometry;
use crate::shaderpack::geometry::GeometryCategory;
use bevy::prelude::Entity;
use bevy::render::render_phase::TrackedRenderPass;

impl ShaderpackGeometry {
    /// Records `category` for `view` (a render-world view entity with visible chunks): its
    /// camera list, or with `shadow` its shadow-pass list from
    /// [`Self::prepare_shadow_view`]. The pass must already hold a pipeline built with
    /// [`super::super::vertex_buffer_layouts`] and the program's bind groups. Returns the draws
    /// issued.
    pub fn draw<'w>(
        &'w self,
        pass: &mut TrackedRenderPass<'w>,
        category: GeometryCategory,
        view: Entity,
        shadow: bool,
    ) -> u32 {
        let Some(index_buffer) = &self.index_buffer else {
            return 0;
        };
        let lists = if shadow { &self.shadows } else { &self.views };
        let Some(draws) = lists.get(&view) else {
            return 0;
        };
        let origins = self.draw_origins.as_ref().filter(|_| self.layout.packed());
        draws.record(
            pass,
            category,
            &self.pages,
            index_buffer,
            &self.constants,
            origins,
        )
    }

    /// Visits triangle inputs from this view's retained camera or shadow lists.
    pub(crate) fn emulation_inputs<'a>(
        &'a self,
        category: GeometryCategory,
        view: Entity,
        shadow: bool,
        visit: &mut dyn FnMut(crate::shaderpack::geometry_shader::Input<'a>),
    ) {
        let Some(indices) = &self.index_buffer else {
            return;
        };
        let lists = if shadow { &self.shadows } else { &self.views };
        if let Some(draws) = lists.get(&view) {
            draws.emulation_inputs(
                category,
                &self.pages,
                indices,
                self.draw_origins.as_ref().filter(|_| self.layout.packed()),
                visit,
            );
        }
    }
}
