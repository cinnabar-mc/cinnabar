use super::*;
use particles::EmptyWorld;

/// Supplies one-particle emitters for the fishing effects so spawns are kept without game assets.
fn system() -> ParticleSystem {
    let mut system = ParticleSystem::default();
    for identifier in [
        assets::facts::ParticleEffect::BasicBubbleParticle.identifier(),
        assets::facts::ParticleEffect::WaterWakeParticle.identifier(),
    ] {
        let effect = format!(
            r#"{{"particle_effect":{{"description":{{"identifier":"{identifier}","basic_render_parameters":{{"material":"particles_alpha","texture":"test"}}}},"components":{{"minecraft:emitter_rate_instant":{{"num_particles":1}},"minecraft:emitter_lifetime_once":{{"active_time":0.1}},"minecraft:emitter_shape_point":{{}},"minecraft:particle_lifetime_expression":{{"max_lifetime":1}},"minecraft:particle_appearance_billboard":{{"size":[0.05,0.05]}}}}}}}}"#
        );
        assert!(system.register_effect(effect.as_bytes()));
    }
    system
}

#[test]
fn fish_bite_scatters_bubbles_and_wake_at_the_hook() {
    let stream = super::totem_tests::stream();
    let mut system = system();
    let registry = sim::CollisionRegistry::default();
    let world = StreamParticleWorld::new(&stream, &registry, None);
    let routing = Routing {
        world: &world,
        stream: &stream,
        mode: NetworkIdMode::Sequential,
        icons: None,
    };
    route_notice(
        &mut system,
        &routing,
        &ActorStatusNotice {
            runtime_id: 42,
            kind: ActorStatusKind::FishHookHookTime,
            data: 0,
            position: [0.0; 3],
            height: None,
            eating_position: [0.0; 3],
            fish_position: None,
        },
    );
    system.tick(0.001, &EmptyWorld);
    assert!(system.live_particles() > 0);
}
