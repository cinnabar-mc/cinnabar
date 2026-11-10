//! The actor material type Bedrock selects from a material's `fragmentShader`.
//!
//! `getActorVariation` names an actor draw's material from a table indexed by this type. The
//! type comes from the program's stem: the text after the last `/`, up to the first `.` after
//! it. `entity` and `pattern` select their families and one more stem, elided,
//! selects a third. Any other program, including none, logs that the shader cannot be assigned
//! to an entity and draws plain `Actor` with no variant flags.

use super::Resolved;

/// The family of actor materials a program selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActorMaterialType {
    Entity,
    Pattern,
    /// A program no actor type names; the elided third stem is indistinguishable from these.
    Other,
}

/// The type of `resolved`'s actor draws. A material that names no program but inherits one the
/// client cannot see is assumed to draw with the entity program.
pub(super) fn of(resolved: &Resolved) -> ActorMaterialType {
    match resolved.definition.fragment_shader.as_deref() {
        Some(program) => from_program(program),
        None if resolved.assumes_entity_program => ActorMaterialType::Entity,
        None => ActorMaterialType::Other,
    }
}

/// The type `program` selects by its stem.
pub(super) fn from_program(program: &str) -> ActorMaterialType {
    let name = program.rsplit_once('/').map_or(program, |(_, name)| name);
    match name.split_once('.').map(|(stem, _)| stem) {
        Some("entity") => ActorMaterialType::Entity,
        Some("pattern") => ActorMaterialType::Pattern,
        _ => ActorMaterialType::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_stem_selects_the_actor_material_type() {
        use ActorMaterialType::{Entity, Other, Pattern};
        for (program, expected) in [
            ("shaders/entity.fragment", Entity),
            ("entity.fragment", Entity),
            ("shaders/glsl/pattern.fragment.bin", Pattern),
            ("shaders/entity", Other),
            ("shaders/.fragment", Other),
            ("shaders/Entity.fragment", Other),
            ("shaders/banner.fragment", Other),
            ("entity/shaders/plain.fragment", Other),
        ] {
            assert_eq!(from_program(program), expected, "{program}");
        }
    }
}
