//! Fragment resources shared by production hand drawing and native raster fixtures.

use wgpu::{BindGroupEntry, BindingResource, Buffer, Sampler, TextureView};

/// Keeps Bedrock hand artwork in gamma space until lighting is composed.
pub(crate) fn hand_artwork_format(light: &render_model::HandRigLight) -> wgpu::TextureFormat {
    if light.java_lights[0][3] == 0.0 {
        wgpu::TextureFormat::Rgba8Unorm
    } else {
        wgpu::TextureFormat::Rgba8UnormSrgb
    }
}

/// Bedrock SDR hands blend over the scene's gamma bytes through its compatible raw view.
pub(super) fn hand_uses_encoded_color(light: &render_model::HandRigLight, hdr: bool) -> bool {
    !hdr && light.java_lights[0][3] == 0.0
}

pub(crate) struct HandFragmentBindings<'a> {
    pub(crate) instances: &'a Buffer,
    pub(crate) skins: &'a TextureView,
    pub(crate) skin_sampler: &'a Sampler,
    pub(crate) material: &'a Buffer,
    pub(crate) light: &'a Buffer,
    pub(crate) main_atlas: &'a TextureView,
    pub(crate) offhand_atlas: &'a TextureView,
    pub(crate) map_background: &'a TextureView,
    pub(crate) item_glint: &'a TextureView,
    pub(crate) glint_sampler: &'a Sampler,
}

impl<'a> HandFragmentBindings<'a> {
    /// Binds every resource read by the hand fragment, including item glint instance words.
    pub(crate) fn entries(self) -> [BindGroupEntry<'a>; 10] {
        [
            BindGroupEntry {
                binding: 1,
                resource: self.instances.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::TextureView(self.skins),
            },
            BindGroupEntry {
                binding: 7,
                resource: BindingResource::Sampler(self.skin_sampler),
            },
            BindGroupEntry {
                binding: 8,
                resource: self.material.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 9,
                resource: self.light.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 10,
                resource: BindingResource::TextureView(self.main_atlas),
            },
            BindGroupEntry {
                binding: 11,
                resource: BindingResource::TextureView(self.offhand_atlas),
            },
            BindGroupEntry {
                binding: 14,
                resource: BindingResource::TextureView(self.map_background),
            },
            BindGroupEntry {
                binding: 12,
                resource: BindingResource::TextureView(self.item_glint),
            },
            BindGroupEntry {
                binding: 13,
                resource: BindingResource::Sampler(self.glint_sampler),
            },
        ]
    }
}
