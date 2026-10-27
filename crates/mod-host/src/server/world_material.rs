//! The `world-material` import: stages world material declarations and bindings, which the
//! client re-validates before it draws them.

use super::{Command, Result, State, cinnabar::server_experience::world_material as wit};
use server_experience::world_material::{
    WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec, WorldTarget,
};

/// The previous value of one staged declaration, which restores it.
pub(super) enum Undo {
    Material(u32, Option<WorldMaterialSpec>),
    Binding(u32, Option<WorldBinding>),
}

impl State {
    /// Applies `command`'s world declaration in place, checked against the bundle's memory,
    /// and records how to undo it; does nothing for other commands. A refused declaration
    /// changes nothing.
    pub(super) fn stage_world(&mut self, command: &Command) -> Result<(), String> {
        let memory = self.capabilities.scope.memory_bytes;
        let undo = match command {
            Command::WorldMaterial { id, material } => self
                .world
                .set_material_within(*id, material.as_ref(), memory)
                .map(|previous| Undo::Material(*id, previous)),
            Command::WorldBinding { id, binding } => self
                .world
                .set_binding_within(*id, binding.as_ref(), memory)
                .map(|previous| Undo::Binding(*id, previous)),
            _ => return Ok(()),
        };
        self.world_undo
            .push(undo.map_err(|error| error.to_string())?);
        Ok(())
    }

    /// Undoes the declarations staged since the last committed transaction, newest first.
    /// Each restores a state the declarations were in before, so none can be refused.
    pub(super) fn roll_back_world(&mut self) {
        while let Some(undo) = self.world_undo.pop() {
            let restored = match undo {
                Undo::Material(id, previous) => {
                    self.world.set_material(id, previous.as_ref()).map(drop)
                }
                Undo::Binding(id, previous) => {
                    self.world.set_binding(id, previous.as_ref()).map(drop)
                }
            };
            debug_assert!(restored.is_ok(), "an undo restores a held state");
        }
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
            .world
            .materials()
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
