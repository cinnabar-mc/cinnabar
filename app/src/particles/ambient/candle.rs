//! Fixed-tick lit-candle smoke and flames. The candle's ambient sound is not voiced yet.

use chunk_pipeline::WorldStream;
use particles::{ParticleSystem, SpawnRequest, ambient::emit_candle_effects};

use super::AmbientParticles;

impl AmbientParticles {
    /// Emits the effects of the candle block `runtime_id` at `block`, which the caller
    /// identified; unlit states consume no rolls.
    pub(super) fn try_candle(
        &mut self,
        block: [i32; 3],
        runtime_id: u32,
        stream: &WorldStream,
        system: &mut ParticleSystem,
    ) {
        let flames = assets::block_presentation_state::pinned_block_presentation_states()
            .get(stream.network_id_mode(), runtime_id)
            .candle_flames;
        emit_candle_effects(
            block,
            flames.origins(),
            &mut self.random,
            |effect, position| {
                system.spawn(&request(effect, position));
            },
        );
    }
}

/// One manual particle with the native callback's zero velocity.
fn request(effect: &str, position: [f32; 3]) -> SpawnRequest {
    SpawnRequest {
        effect: effect.to_owned(),
        position,
        inherit_velocity: Some([0.0; 3]),
        manual_count: Some(1),
        ..SpawnRequest::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candle_callback_requests_one_stationary_particle_without_overriding_the_pack() {
        let request = request(particles::ambient::CANDLE_FLAME_EFFECT, [-3.5, 64.5, 2.5]);
        assert_eq!(request.position, [-3.5, 64.5, 2.5]);
        assert_eq!(request.manual_count, Some(1));
        assert_eq!(request.inherit_velocity, Some([0.0; 3]));
        assert!(request.variables.is_empty());
        assert!(request.basis.is_none());
    }
}
