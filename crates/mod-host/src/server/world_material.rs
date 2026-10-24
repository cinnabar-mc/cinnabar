//! The `world-material` import: stages world material declarations and bindings, which the
//! client re-validates before it draws them.

use super::{Command, Result, State, cinnabar::server_experience::world_material as wit};
use server_experience::world_material::{
    WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec, WorldTarget,
};

impl wit::Host for State {
    /// Stages material `id` after the shader and its declared resources pass validation.
    fn set_material(&mut self, id: u32, spec: wit::MaterialSpec) -> Result<Result<(), String>> {
        let spec = WorldMaterialSpec {
            source: spec.source,
            textures: spec.textures,
            params: spec.params,
            depth: match spec.depth {
                wit::Depth::Test => WorldDepth::Test,
                wit::Depth::Always => WorldDepth::Always,
            },
            blend: match spec.blend {
                wit::Blend::Opaque => WorldBlend::Opaque,
                wit::Blend::Cutout => WorldBlend::Cutout,
                wit::Blend::Premultiplied => WorldBlend::Premultiplied,
                wit::Blend::Additive => WorldBlend::Additive,
            },
        };
        self.stage(Command::WorldMaterial {
            id,
            material: Some(spec),
        })
    }

    fn remove_material(&mut self, id: u32) -> Result<Result<(), String>> {
        self.stage(Command::WorldMaterial { id, material: None })
    }

    fn bind(&mut self, id: u32, target: wit::Target, material: u32) -> Result<Result<(), String>> {
        let target = match target {
            wit::Target::Entity(identifier) => WorldTarget::Entity(identifier),
            wit::Target::Block(identifier) => WorldTarget::Block(identifier),
        };
        self.stage(Command::WorldBinding {
            id,
            binding: Some(WorldBinding { target, material }),
        })
    }

    fn unbind(&mut self, id: u32) -> Result<Result<(), String>> {
        self.stage(Command::WorldBinding { id, binding: None })
    }
}
