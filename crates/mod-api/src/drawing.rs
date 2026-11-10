//! Authority and lifetimes for the versioned world drawing interface.

/// Generated resource operations and records from the world drawing WIT contract.
pub use crate::bindings::cinnabar::world_draw::drawing as api;

pub mod shapes;

/// Per-owner cells across block grids, enough for a 3 by 3 chunk view 256 blocks high.
pub const MAX_BLOCK_CELLS: usize = 2 * 1024 * 1024;
/// Largest dimension of a block grid; the total cell allowance also applies.
pub const MAX_BLOCK_EXTENT: u32 = 384;
/// Distinct resolved states in a block-grid palette.
pub const MAX_BLOCK_PALETTE: usize = 4096;
/// UTF-8 bytes in one palette's named state descriptions.
pub const MAX_BLOCK_STATE_BYTES: usize = 256 * 1024;
/// Block-grid input bytes admitted in one callback, separate from ordinary drawing uploads.
pub const MAX_BLOCK_MUTATION_BYTES: usize = 16 * 1024 * 1024;
/// Mesh triangles in one generation, including model blocks and liquids.
pub const MAX_BLOCK_TRIANGLES: usize = 2 * 1024 * 1024;
/// Submitted block triangles across an owner's placements.
pub const MAX_BLOCK_DRAW_TRIANGLES: usize = MAX_BLOCK_TRIANGLES * 2;
/// Packed terrain mesh bytes in one generation.
pub const MAX_BLOCK_MESH_BYTES: usize = 128 * 1024 * 1024;
/// GPU bytes for an owner's block arenas, orders and placement uniforms.
pub const MAX_BLOCK_GPU_BYTES: u64 = 256 * 1024 * 1024;
/// Simultaneously retained clip resources per owner.
pub const MAX_DRAW_CLIPS: usize = 8;
/// Maximum private clip attachment storage across an owner's views.
pub const MAX_CLIP_ATTACHMENT_BYTES: u64 = 128 * 1024 * 1024;
/// Two coverage levels allow one clip nested inside another.
pub const MAX_DRAW_CLIP_DEPTH: usize = 2;

/// Largest solved sky or block light channel.
pub const MAX_BLOCK_LIGHT: u8 = 15;

/// Encodes a palette index and optional solved lights for a compact dense grid upload.
pub fn pack_block_cell(cell: api::BlockCell) -> Option<u32> {
    if cell.palette as usize >= MAX_BLOCK_PALETTE
        || cell.sky_light.is_some_and(|v| v > MAX_BLOCK_LIGHT)
        || cell.block_light.is_some_and(|v| v > MAX_BLOCK_LIGHT)
    {
        return None;
    }
    Some(
        cell.palette
            | (u32::from(cell.sky_light.unwrap_or(u8::MAX)) << u16::BITS)
            | (u32::from(cell.block_light.unwrap_or(u8::MAX)) << (u16::BITS + u8::BITS)),
    )
}

/// Decodes the canonical grid word and rejects unknown light codes or palette slots.
pub fn unpack_block_cell(word: u32) -> Option<api::BlockCell> {
    let sky = (word >> u16::BITS) as u8;
    let block = (word >> (u16::BITS + u8::BITS)) as u8;
    let cell = api::BlockCell {
        palette: u32::from(word as u16),
        sky_light: (sky != u8::MAX).then_some(sky),
        block_light: (block != u8::MAX).then_some(block),
    };
    pack_block_cell(cell).filter(|packed| *packed == word)?;
    Some(cell)
}

/// Native retained-instance storage, checked against the renderer's typed record.
pub const INSTANCE_GPU_BYTES: usize =
    (16 + 4 + 4 + crate::materials::PARAMS) * std::mem::size_of::<f32>();

/// Fixed host tessellation for a round join or semicircular end cap.
pub const ROUND_SEGMENTS: usize = 8;

/// Bounds expanded GPU vertices for all selectable styles, saturating on count overflow.
/// Callers validate topology separately; repeated polyline points still consume this budget.
pub fn expanded_vertices(vertices: usize, indices: usize, polyline: bool, closed: bool) -> usize {
    if !polyline {
        return indices;
    }
    let segments = vertices.saturating_sub(usize::from(!closed));
    segments
        .saturating_mul(6 + ROUND_SEGMENTS * 3)
        .saturating_add(if closed { 0 } else { ROUND_SEGMENTS * 6 })
}

/// Places retained geometry with depth testing, white tint and an identity transform.
/// Set width and width mode explicitly when a polyline needs a different stroke.
pub fn instance(geometry: api::Handle, anchor: api::Anchor) -> api::Instance {
    api::Instance {
        geometry,
        material: None,
        clip: None,
        params: crate::materials::Preset::default().0.to_vec(),
        anchor,
        transform: api::Transform {
            translation: (0., 0., 0.),
            rotation: (0., 0., 0., 1.),
            scale: (1., 1., 1.),
        },
        tint: (1., 1., 1., 1.),
        depth: api::Depth::Test,
        blend: api::Blend::Premultiplied,
        width: 1.,
        width_mode: api::WidthMode::World,
        join: api::Join::Bevel,
        cap: api::Cap::Butt,
    }
}

macro_rules! permissions {
    ($($field:ident),+ $(,)?) => {
        /// Independently consented drawing authority. Absence always denies a capability.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub struct DrawingPermissions { $(pub $field: bool,)+ }
        impl DrawingPermissions {
            /// Canonical capability names, generated from their owning fields.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($field),)+];
            /// Host ceiling for an explicitly local world; user consent still applies.
            pub const ALL: Self = Self { $($field: true,)+ };
            /// Retains only authority independently granted by both inputs.
            pub const fn intersect(self, other: Self) -> Self { Self { $($field: self.$field && other.$field,)+ } }
            /// Reads one canonical permission name; unknown names never grant authority.
            pub fn enabled(self, name: &str) -> bool { match name { $(stringify!($field) => self.$field,)+ _ => false } }
            /// Intersects host limits, package request, user consent and current session policy.
            pub const fn effective(host: Self, requested: Self, consent: Self, session: Self) -> Self {
                host.intersect(requested).intersect(consent).intersect(session)
            }
        }
    };
}
permissions! { world_draw, entity_observe, entity_highlight, render_through_walls, render_material, render_post }

/// A token belongs to one owner incarnation; slots never authorize another incarnation.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct DrawingHandle {
    pub owner: u64,
    pub generation: u64,
    pub slot: u64,
}

/// Trusted connection and dimension fence, independent of a server address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DrawingSession {
    pub connection: u64,
    pub dimension: i32,
    /// Set only by the trusted local-world launcher.
    pub local: bool,
    pub permissions: DrawingPermissions,
}

/// Retained union selections per component, including empty selections.
pub const MAX_MASKS: usize = 8;
/// Distinct actors selected by one mask.
pub const MAX_MASK_ENTITIES: usize = 32;
/// Actor vertices admitted to one selection, including equipment.
pub const MAX_MASK_VERTICES: u64 = 262_144;
/// Total attachment bytes across masks and views, including MSAA resolve targets.
pub const MAX_MASK_ATTACHMENT_BYTES: u64 = 256 * 1024 * 1024;
/// Mask imports admitted in one callback.
pub const MAX_MASK_CALLS: u32 = 64;
/// Maximum physical-pixel expansion of a crisp host outline.
pub const MAX_MASK_OUTLINE_RADIUS: f32 = 4.0;
/// Downsample levels shared by every effect reading a selection's halo.
pub const MASK_BLUR_LEVELS: usize = 4;
/// Maximum physical-pixel radius of a host-composed selection halo.
pub const MAX_MASK_GLOW_RADIUS: f32 = (1 << MASK_BLUR_LEVELS) as f32;

/// Bounded host composition; colour components use the display's sRGB encoding.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[f32; 4]>,
    pub outline: [f32; 4],
    pub outline_radius: f32,
    pub glow: [f32; 4],
    pub glow_radius: f32,
}

impl MaskStyle {
    /// Rejects non-finite colours, coverage or radii before retaining presentation state.
    pub fn valid(self) -> bool {
        self.fill.is_none_or(|fill| {
            fill.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        }) && self
            .outline
            .iter()
            .chain(&self.glow)
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            && (0.0..=MAX_MASK_OUTLINE_RADIUS).contains(&self.outline_radius)
            && (0.0..=MAX_MASK_GLOW_RADIUS).contains(&self.glow_radius)
    }
}

/// An actor incarnation, shared with trusted frame publication; never disclosed to shaders.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct MaskEntity {
    pub connection: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
}

/// Draw-time depth authority cannot be changed after mask creation.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MaskDepth {
    Test,
    Always,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_instance_uses_the_generated_contract_and_safe_defaults() {
        let handle = api::Handle {
            owner: 1,
            generation: 2,
            slot: 3,
        };
        let spec = instance(handle, api::Anchor::Camera);
        assert_eq!(
            (
                spec.geometry.owner,
                spec.geometry.generation,
                spec.geometry.slot
            ),
            (1, 2, 3)
        );
        assert!(matches!(spec.anchor, api::Anchor::Camera));
        assert_eq!(spec.transform.translation, (0., 0., 0.));
        assert_eq!(spec.transform.rotation, (0., 0., 0., 1.));
        assert_eq!(spec.transform.scale, (1., 1., 1.));
        assert_eq!(spec.tint, (1., 1., 1., 1.));
        assert_eq!(spec.depth, api::Depth::Test);
        assert_eq!(spec.blend, api::Blend::Premultiplied);
        assert_eq!(spec.width, 1.);
        assert_eq!(spec.width_mode, api::WidthMode::World);
        assert_eq!(spec.join, api::Join::Bevel);
        assert_eq!(spec.cap, api::Cap::Butt);
    }

    #[test]
    fn every_authority_input_can_deny() {
        let all = DrawingPermissions::ALL;
        let denied = DrawingPermissions::default();
        for missing in 0..4 {
            let mut inputs = [all; 4];
            inputs[missing] = denied;
            assert_eq!(
                DrawingPermissions::effective(inputs[0], inputs[1], inputs[2], inputs[3]),
                denied
            );
        }
        assert_eq!(DrawingPermissions::effective(all, all, all, all), all);
    }
    #[test]
    fn packed_grid_cells_round_trip_all_solved_lights_and_reject_reserved_codes() {
        for palette in [0, (MAX_BLOCK_PALETTE - 1) as u32] {
            for sky in (0..=MAX_BLOCK_LIGHT).map(Some).chain([None]) {
                for block in (0..=MAX_BLOCK_LIGHT).map(Some).chain([None]) {
                    let word = pack_block_cell(api::BlockCell {
                        palette,
                        sky_light: sky,
                        block_light: block,
                    })
                    .unwrap();
                    let cell = unpack_block_cell(word).unwrap();
                    assert_eq!(
                        (cell.palette, cell.sky_light, cell.block_light),
                        (palette, sky, block)
                    );
                }
            }
        }
        assert!(unpack_block_cell(16 << u16::BITS).is_none());
        assert!(unpack_block_cell(MAX_BLOCK_PALETTE as u32).is_none());
    }
}
