//! The `world-material` import: stages world material declarations and bindings, which the
//! client re-validates before it draws them.

use super::{Command, Result, State, cinnabar::server_experience::world_material as wit};
use server_experience::world_material::{
    WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec, WorldTarget,
};

impl State {
    /// The world materials `command` would leave staged, checked against the bundle's memory,
    /// or `None` when it declares no world material or binding.
    pub(super) fn stage_world(
        &self,
        command: &Command,
    ) -> Result<Option<server_experience::world_material::WorldMaterials>, String> {
        let mut staged = self.world_staged.clone();
        let changed = match command {
            Command::WorldMaterial { id, material } => staged.set_material(*id, material.as_ref()),
            Command::WorldBinding { id, binding } => staged.set_binding(*id, binding.as_ref()),
            _ => return Ok(None),
        };
        changed
            .and_then(|()| staged.check(self.capabilities.scope.memory_bytes))
            .map_err(|error| error.to_string())?;
        Ok(Some(staged))
    }

    /// Validates the shader within the per-callback compile budget, after the declaration's
    /// own bounds, so a rejected source still spends an attempt. Redeclaring material `id`
    /// with only new parameters revalidates nothing and spends no compile.
    fn compile_world_material(&mut self, id: u32, spec: &WorldMaterialSpec) -> Result<(), String> {
        self.capabilities
            .world_authority()
            .validate_material(spec)
            .map_err(|error| error.to_string())?;
        if self
            .world_staged
            .materials
            .get(&id)
            .is_some_and(|staged| staged.same_program(spec))
        {
            return Ok(());
        }
        if self.world_compiles >= mod_api::materials::MAX_WORLD_MATERIAL_COMPILES_PER_CALLBACK {
            return Err("world material compile budget exceeded".into());
        }
        self.world_compiles += 1;
        mod_render::world_material::compose(&spec.source, spec.textures.len()).map(|_| ())
    }
}

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
        if let Err(error) = self.compile_world_material(id, &spec) {
            self.charge()?;
            return Ok(Err(error));
        }
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
