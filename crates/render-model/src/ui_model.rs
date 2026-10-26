//! Small bone palettes animate retained UI geometry without replacing its vertices.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

use crate::{UiRenderVertex, UiScissor};

/// Low instance-index bits hold the UI texture-array layer; upper bits select a bone palette.
pub const UI_MODEL_LAYER_BITS: u32 = crate::MAX_UI_TEXTURE_LAYERS.ilog2();
const _: () = assert!(crate::MAX_UI_TEXTURE_LAYERS.is_power_of_two());

/// One model-space bone maps points to physical UI coordinates and normals to the light frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct UiModelBone {
    pub screen: [[f32; 4]; 4],
    pub normal: [[f32; 4]; 3],
    pub overlay: [f32; 4],
    pub uv_rect: [f32; 4],
    /// Cross-axis terms retain the texture orientation of an effect triangle.
    pub uv_cross: [f32; 4],
    pub tint: [f32; 4],
    pub styles: [u32; 4],
}

impl UiModelBone {
    /// Checks the floating payload without interpreting the trailing style bits as floats.
    pub fn is_finite(&self) -> bool {
        const FLOAT_WORDS: usize =
            std::mem::offset_of!(UiModelBone, styles) / std::mem::size_of::<f32>();
        const _: () = assert!(
            std::mem::offset_of!(UiModelBone, styles) + std::mem::size_of::<[u32; 4]>()
                == std::mem::size_of::<UiModelBone>()
        );
        bytemuck::cast_slice::<_, f32>(std::slice::from_ref(self))[..FLOAT_WORDS]
            .iter()
            .all(|value| value.is_finite())
    }

    /// Resolves a retained vertex for offline rasterizers and shader conformance checks.
    pub fn project(&self, mut vertex: UiRenderVertex) -> UiRenderVertex {
        let point = Mat4::from_cols_array_2d(&self.screen)
            * Vec4::new(vertex.position[0], vertex.position[1], vertex.clip_z, 1.0);
        vertex.position = [point.x, point.y];
        vertex.clip_z = point.z;
        vertex.clip_w = point.w;
        vertex.uv = [0, 1].map(|axis| {
            self.uv_rect[axis]
                + vertex.uv[axis] * (self.uv_rect[axis + 2] - self.uv_rect[axis])
                + vertex.uv[1 - axis] * self.uv_cross[axis]
        });
        vertex.color = [0, 1, 2, 3]
            .map(|channel| (f32::from(vertex.color[channel]) * self.tint[channel]).round() as u8);
        let source = Vec3::from_slice(&vertex.overlay_color[..3]);
        let normal = Vec3::new(
            Vec3::from_slice(&self.normal[0][..3]).dot(source),
            Vec3::from_slice(&self.normal[1][..3]).dot(source),
            Vec3::from_slice(&self.normal[2][..3]).dot(source),
        );
        vertex.model_light = normal.try_normalize().map_or(vertex.model_light, |normal| {
            render_api::fancy_actor_shade(normal.to_array(), 0.0)
        });
        vertex.overlay_color = self.overlay;
        vertex.style_flags =
            (vertex.style_flags | self.styles[0]) & !render_api::UI_STYLE_MODEL_VERTEX;
        vertex
    }
}

/// A control's ordered depth scope owns its palette and conservative physical-pixel coverage.
#[derive(Clone, Debug, PartialEq)]
pub struct UiModelPose {
    pub scope: u32,
    pub bones: Arc<[UiModelBone]>,
    pub bounds: UiScissor,
}

/// Selects a model palette and texture layer through the ordinary UI draw's instance index.
pub fn ui_model_instance(models: &[UiModelPose], scope: Option<u32>, layer: u32) -> u32 {
    let mut offset = 0;
    for model in models {
        if Some(model.scope) == scope {
            return ((offset + 1) << UI_MODEL_LAYER_BITS) | layer;
        }
        offset += model.bones.len() as u32;
    }
    layer
}

/// Uses the same palette lookup as a GPU model draw; ordinary vertices pass through unchanged.
pub fn project_ui_model_vertex(
    vertex: UiRenderVertex,
    pose: Option<&UiModelPose>,
) -> Option<UiRenderVertex> {
    if vertex.style_flags & render_api::UI_STYLE_MODEL_VERTEX == 0 {
        return Some(vertex);
    }
    pose?
        .bones
        .get((vertex.clip_w as usize).checked_sub(1)?)
        .map(|bone| bone.project(vertex))
}

#[cfg(test)]
mod tests;
