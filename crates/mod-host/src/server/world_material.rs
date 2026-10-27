//! The `world-material` and `world-material-effects` imports: stage world material
//! declarations, bindings and compute declarations, which the client re-validates before it
//! draws them.

use super::{
    Command, Result, State,
    cinnabar::server_experience::{world_material as wit, world_material_effects as effects},
};
use server_experience::world_material::{
    WorldBinding, WorldBlend, WorldBloom, WorldComputeSpec, WorldDepth, WorldEffects,
    WorldMaterialSpec, WorldOutline, WorldParticles, WorldTarget, WorldUndo,
};

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
                .map(|previous| WorldUndo::Material(*id, previous)),
            Command::WorldBinding { id, binding } => self
                .world
                .set_binding_within(*id, binding.as_ref(), memory)
                .map(|previous| WorldUndo::Binding(*id, previous)),
            Command::WorldCompute { id, compute } => self
                .world
                .set_compute_within(*id, compute.as_ref(), memory)
                .map(|previous| WorldUndo::Compute(*id, previous)),
            _ => return Ok(()),
        };
        self.world_undo
            .push(undo.map_err(|error| error.to_string())?);
        Ok(())
    }

    /// Undoes the declarations staged since the last committed transaction, newest first.
    pub(super) fn roll_back_world(&mut self) {
        while let Some(undo) = self.world_undo.pop() {
            self.world.undo(undo);
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
        self.spend_world_compile()?;
        mod_render::world_material::compose_with(
            &spec.source,
            spec.textures.len(),
            features(&spec.effects),
        )
        .map(|_| ())
    }

    /// Spends one of this callback's world shader validations.
    fn spend_world_compile(&mut self) -> Result<(), String> {
        if self.world_compiles >= mod_api::materials::MAX_WORLD_MATERIAL_COMPILES_PER_CALLBACK {
            return Err("world material compile budget exceeded".into());
        }
        self.world_compiles += 1;
        Ok(())
    }

    /// Validates and stages material `id`.
    fn declare_world_material(
        &mut self,
        id: u32,
        spec: WorldMaterialSpec,
    ) -> Result<Result<(), String>> {
        if let Err(error) = self.compile_world_material(id, &spec) {
            self.charge()?;
            return Ok(Err(error));
        }
        self.stage(Command::WorldMaterial {
            id,
            material: Some(spec),
        })
    }

    /// Validates the compute within the per-callback compile budget, after the declaration's
    /// own bounds; new parameters alone revalidate nothing.
    fn compile_world_compute(&mut self, id: u32, spec: &WorldComputeSpec) -> Result<(), String> {
        self.capabilities
            .world_authority()
            .validate_compute(spec)
            .map_err(|error| error.to_string())?;
        if self
            .world
            .computes()
            .get(&id)
            .is_some_and(|staged| staged.same_program(spec))
        {
            return Ok(());
        }
        self.spend_world_compile()?;
        mod_render::world_material::compute::compose(&spec.source).map(|_| ())
    }
}

/// The module features a material's effects call: each effect's guest functions and the
/// resources they read.
pub fn features(effects: &WorldEffects) -> mod_render::world_material::Features {
    mod_render::world_material::Features {
        displace: effects.displacement_blocks.is_some(),
        scene: effects.scene,
        cells: effects.cells.is_some(),
        outline: effects.outline.is_some(),
        particles: effects.particles.is_some(),
    }
}

/// A material declaration from its WIT form.
fn material_spec(spec: wit::MaterialSpec, effects: WorldEffects) -> WorldMaterialSpec {
    WorldMaterialSpec {
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
        effects,
    }
}

impl wit::Host for State {
    /// Stages material `id` after the shader and its declared resources pass validation.
    fn set_material(&mut self, id: u32, spec: wit::MaterialSpec) -> Result<Result<(), String>> {
        self.declare_world_material(id, material_spec(spec, WorldEffects::default()))
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

impl effects::Host for State {
    /// Stages material `id` with `declared` effects after the shader, its declared resources
    /// and the effects' permissions and ranges pass validation.
    fn set_material_with_effects(
        &mut self,
        id: u32,
        spec: wit::MaterialSpec,
        declared: effects::Effects,
    ) -> Result<Result<(), String>> {
        let effects = WorldEffects {
            displacement_blocks: declared.displacement_blocks,
            scene: declared.scene,
            outline: declared.outline.map(|outline| WorldOutline {
                width_pixels: outline.width_pixels,
            }),
            bloom: declared.bloom.map(|bloom| WorldBloom {
                strength: bloom.strength,
                radius_pixels: bloom.radius_pixels,
            }),
            particles: declared.particles.map(|particles| WorldParticles {
                count: particles.count,
                radius_blocks: particles.radius_blocks,
            }),
            cells: declared.cells,
        };
        self.declare_world_material(id, material_spec(spec, effects))
    }

    /// Stages compute `id` after its program and cell count pass validation.
    fn set_compute(&mut self, id: u32, spec: effects::ComputeSpec) -> Result<Result<(), String>> {
        let spec = WorldComputeSpec {
            source: spec.source,
            cells: spec.cells,
            params: spec.params,
        };
        if let Err(error) = self.compile_world_compute(id, &spec) {
            self.charge()?;
            return Ok(Err(error));
        }
        self.stage(Command::WorldCompute {
            id,
            compute: Some(spec),
        })
    }

    fn remove_compute(&mut self, id: u32) -> Result<Result<(), String>> {
        self.stage(Command::WorldCompute { id, compute: None })
    }
}

#[cfg(test)]
mod tests;
