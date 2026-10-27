//! The session's programmable world materials, as the main world publishes them to the
//! renderer.

use bevy::prelude::Resource;
use mod_render::world_material::publication::{
    Blend, Depth, WorldMaterialParams, WorldMaterialSet,
};
use std::sync::{Arc, Mutex, PoisonError};

/// The current world material set; replaced only when it changes, so an unchanged session
/// extracts the same allocation.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldMaterialScene {
    pub set: Arc<WorldMaterialSet>,
}

/// Each drawn material's parameters, published apart from the set so a parameter update
/// neither registers modules nor re-resolves bound blocks.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldMaterialParamScene {
    pub params: Arc<WorldMaterialParams>,
}

/// The block slots whose tag some resident terrain mesh carries, one bit per slot, which the
/// render world observes each frame and the main world reads before it hands a slot to
/// another bundle, with the number of observations so far.
#[derive(Resource, Clone)]
pub struct WorldMaterialCarried(Arc<Mutex<(u64, u64)>>);

impl Default for WorldMaterialCarried {
    /// Every slot counts as carried until the render world has looked.
    fn default() -> Self {
        Self(Arc::new(Mutex::new((0, u64::MAX))))
    }
}

impl WorldMaterialCarried {
    /// How many observations the render world has made, and the slots carried at the last.
    pub fn get(&self) -> (u64, u64) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records one observation: the slots resident meshes carry.
    pub fn set(&self, slots: u64) {
        let mut carried = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        *carried = (carried.0 + 1, slots);
    }
}

impl WorldMaterialScene {
    /// The tag (one more than the slot) and host raster state an entity `identifier`'s draws
    /// take, if bound.
    pub fn entity(&self, identifier: &str) -> Option<(u8, render_api::render_state::RenderState)> {
        let (slot, published) = self.set.entity(identifier)?;
        let tag = u8::try_from(slot + 1).ok()?;
        Some((tag, host_raster(published.depth, published.blend)))
    }
}

/// The raster state the host draws a world material class with: the guest chooses only the
/// class, never factors, comparisons or writes. Blended classes draw in the sorted pass without
/// depth writes; `Always` passes regardless of stored depth and writes none.
pub fn host_raster(depth: Depth, blend: Blend) -> render_api::render_state::RenderState {
    use render_api::render_state::{
        BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, RenderState,
    };
    let mut raster = RenderState::default();
    let component = |destination| BlendComponent {
        source: BlendFactor::One,
        destination,
        operation: BlendOperation::Add,
    };
    raster.blend = match blend {
        Blend::Opaque | Blend::Cutout => None,
        Blend::Premultiplied => Some(BlendState {
            color: component(BlendFactor::OneMinusSourceAlpha),
            alpha: component(BlendFactor::OneMinusSourceAlpha),
        }),
        Blend::Additive => Some(BlendState {
            color: component(BlendFactor::One),
            alpha: component(BlendFactor::One),
        }),
    };
    raster.depth.write = raster.blend.is_none();
    if depth == Depth::Always {
        raster.depth.compare = Some(CompareFunction::Always);
        raster.depth.write = false;
    }
    raster
}

#[cfg(any(test, feature = "publication-test-support"))]
impl WorldMaterialScene {
    /// A scene binding each of `entities` to its own plain material, in order.
    pub fn with_bound_entities(entities: &[&str]) -> Self {
        use mod_render::world_material::{WorldMaterial, publication::Published};
        let material = Arc::new(WorldMaterial {
            source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }"
                .into(),
            textures: 0,
        });
        let mut set = WorldMaterialSet {
            revision: 1,
            ..WorldMaterialSet::default()
        };
        for (slot, entity) in entities.iter().enumerate() {
            set.materials.push(Some(Published {
                key: slot as u64 + 1,
                material: Arc::clone(&material),
                textures: Arc::from([]),
                depth: Depth::Test,
                blend: Blend::Opaque,
            }));
            set.entities.insert((*entity).into(), slot as u16);
        }
        Self { set: Arc::new(set) }
    }
}

/// Most world material compile failures one session keeps for its diagnostics.
pub const MAX_WORLD_MATERIAL_FAILURES: usize = 64;

/// Keys of world materials whose native pipelines failed to compile, shared by the render
/// world that observes the failure and the main world that reports it.
#[derive(Resource, Clone, Default)]
pub struct WorldMaterialFailures(Arc<std::sync::Mutex<std::collections::BTreeSet<u64>>>);

impl WorldMaterialFailures {
    /// Records the failure of material `key`; returns whether it was new. At most
    /// [`MAX_WORLD_MATERIAL_FAILURES`] keys are kept.
    pub fn record(&self, key: u64) -> bool {
        let mut failures = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        failures.len() < MAX_WORLD_MATERIAL_FAILURES && failures.insert(key)
    }

    /// Whether material `key` failed to compile.
    pub fn failed(&self, key: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&key)
    }

    /// Forgets every recorded failure, as a session ends.
    pub fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_failures_are_recorded_once_and_bounded() {
        let failures = WorldMaterialFailures::default();
        assert!(failures.record(3));
        assert!(!failures.record(3));
        assert!(failures.failed(3) && !failures.failed(4));
        for key in 10..10 + MAX_WORLD_MATERIAL_FAILURES as u64 {
            failures.record(key);
        }
        assert!(!failures.record(u64::MAX), "the set stays bounded");
        // Clones share one record across the main and render worlds.
        assert!(failures.clone().failed(3));
        // A new session records again.
        failures.clone().clear();
        assert!(!failures.failed(3) && failures.record(u64::MAX));
    }

    #[test]
    fn host_rasters_own_depth_blend_and_pass_per_class() {
        let opaque = host_raster(Depth::Test, Blend::Opaque);
        assert!(opaque.blend.is_none() && opaque.depth.write);
        assert_eq!(opaque, host_raster(Depth::Test, Blend::Cutout));
        let premultiplied = host_raster(Depth::Test, Blend::Premultiplied);
        assert!(premultiplied.blend.is_some() && !premultiplied.depth.write);
        let additive = host_raster(Depth::Test, Blend::Additive).blend.unwrap();
        assert_eq!(
            additive.color.destination,
            render_api::render_state::BlendFactor::One
        );
        let through = host_raster(Depth::Always, Blend::Opaque);
        assert!(through.ignores_stored_depth() && !through.depth.write);
    }
}
