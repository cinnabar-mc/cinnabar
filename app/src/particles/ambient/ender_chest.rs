//! Fixed-tick ender chest particles.

use particles::{
    ParticleSystem, SpawnRequest,
    ambient::{ENDER_CHEST_EFFECT, emit_ender_chest_effects},
};

use super::AmbientParticles;

impl AmbientParticles {
    /// Emits the effects of the ender chest at `block`, which the caller identified.
    pub(super) fn try_ender_chest(&mut self, block: [i32; 3], system: &mut ParticleSystem) {
        emit_ender_chest_effects(block, &mut self.random, |position| {
            system.spawn(&request(position));
        });
    }
}

/// One pack emitter at `position` with no variables, as the native callback requests it.
fn request(position: [f32; 3]) -> SpawnRequest {
    SpawnRequest {
        effect: ENDER_CHEST_EFFECT.to_owned(),
        position,
        ..SpawnRequest::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ender_chest_callback_uses_the_pack_effect_without_overriding_art_or_motion() {
        let request = request([-3.25, 64.5, 2.75]);
        assert_eq!(request.effect, "minecraft:end_chest");
        assert_eq!(request.position, [-3.25, 64.5, 2.75]);
        assert!(request.variables.is_empty());
        assert!(request.manual_count.is_none());
        assert!(request.inherit_velocity.is_none());
        assert!(request.basis.is_none());
    }
}
