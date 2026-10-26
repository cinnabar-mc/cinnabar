//! CPU inputs shared by first-person presentation and the hand pass.
use glam::{Mat4, Vec3};
use std::{mem::size_of, sync::Arc};
/// Instance texture-selector bits shared with the hand shader.
pub const HAND_ITEM_LAYER_FLAG: u32 = 0x8000_0000;

pub const HAND_OFFHAND_LAYER_FLAG: u32 = 0x4000_0000;

/// Selects the shared map paper texture, independently of either hand image.
pub const HAND_MAP_BACKGROUND_FLAG: u32 = 0x0800_0000;

pub const HAND_BLEND_LAYER_FLAG: u32 = 0x2000_0000;

pub const HAND_CUTOUT_LAYER_FLAG: u32 = 0x1000_0000;

pub const HAND_TEXTURE_LAYER_MASK: u32 = !(HAND_ITEM_LAYER_FLAG
    | HAND_OFFHAND_LAYER_FLAG
    | HAND_BLEND_LAYER_FLAG
    | HAND_CUTOUT_LAYER_FLAG
    | HAND_MAP_BACKGROUND_FLAG);

/// Alpha treatment selected from a held block's admitted face materials.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HandItemAlphaMode {
    #[default]
    Opaque,
    Cutout,
    Blend,
}

impl HandItemAlphaMode {
    /// Encodes the item's alpha mode alongside its artwork-array layer.
    pub const fn texture_layer_flag(self) -> u32 {
        match self {
            Self::Opaque => 0,
            Self::Cutout => HAND_CUTOUT_LAYER_FLAG,
            Self::Blend => HAND_BLEND_LAYER_FLAG,
        }
    }
}

/// World light levels and optional Java directional lights in the hand's camera frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HandRigLight {
    pub block_level: u32,
    pub sky_level: u32,
    pub daylight: f32,
    pub pad: u32,
    /// The first direction's W enables Java shading; zero preserves vanilla's material.
    pub java_lights: [[f32; 4]; 2],
    /// Native Z normals in each rig's frame: body, main hand, offhand.
    pub java_normal_axes: [[f32; 4]; 3],
    /// Camera-to-world orientation for posed Bedrock hand normals, stored as rows.
    pub world_from_hand: [[f32; 4]; 3],
    /// Minimum block light for body, main hand, and offhand; the last lane is padding.
    pub emitted_light: [u32; 4],
}

impl Default for HandRigLight {
    fn default() -> Self {
        let identity = Mat4::IDENTITY.to_cols_array_2d();
        Self {
            block_level: 0,
            sky_level: 0,
            daylight: 0.0,
            pad: 0,
            java_lights: [[0.0; 4]; 2],
            java_normal_axes: [[0.0; 4]; 3],
            world_from_hand: [identity[0], identity[1], identity[2]],
            emitted_light: [0; 4],
        }
    }
}

impl HandRigLight {
    /// Sets the camera orientation used to light posed Bedrock hand normals in world space.
    pub fn with_bedrock_lighting(mut self, world_from_hand: Mat4) -> Self {
        let rows = world_from_hand.transpose().to_cols_array_2d();
        self.world_from_hand = [rows[0], rows[1], rows[2]];
        self
    }

    /// Sets Java's two fixed lights after view bob and look rotation, before hand sway.
    pub fn with_java_lighting(mut self, camera_from_light: Mat4) -> Self {
        self.java_lights =
            [Vec3::new(0.2, 1.0, -0.7), Vec3::new(-0.2, 1.0, 0.7)].map(|direction| {
                camera_from_light
                    .transform_vector3(direction.normalize())
                    .extend(1.0)
                    .to_array()
            });
        self.java_normal_axes = [Vec3::Z.extend(0.0).to_array(); 3];
        self
    }
}

const _: () = assert!(size_of::<HandRigLight>() == 160);

/// The equipment atlas page an item instance samples (layer chosen by the instance's
/// `texture_layer` with its top bit set).
#[derive(Clone, Debug)]
pub struct HandItemAtlas {
    pub width: u16,
    pub height: u16,
    pub layers: u32,
    pub rgba8: Arc<[u8]>,
}
