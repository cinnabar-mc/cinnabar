use super::*;
use particles::{EmptyWorld, ParticleView};

// Feeding a baby animal arrives as event 60 after the Feed event; it drew nothing before.
#[test]
fn baby_age_status_emits_the_growth_effect_at_the_actors_body() {
    let stream = super::totem_tests::stream();
    let registry = sim::CollisionRegistry::default();
    let world = StreamParticleWorld::new(&stream, &registry, None);
    let routing = Routing {
        world: &world,
        stream: &stream,
        mode: NetworkIdMode::Sequential,
        icons: None,
    };
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(
        br#"{"particle_effect":{"description":{"identifier":"minecraft:crop_growth_emitter",
        "basic_render_parameters":{"material":"particles_alpha","texture":"test"}},
        "components":{"minecraft:emitter_lifetime_once":{"active_time":1},
        "minecraft:emitter_rate_instant":{"num_particles":15},
        "minecraft:emitter_shape_point":{},
        "minecraft:particle_lifetime_expression":{"max_lifetime":1},
        "minecraft:particle_appearance_billboard":{"size":[0.1,0.1]}}}}"#
    ));
    let position = [1.0, 64.0, 2.0];
    let height = 0.7;
    route_notice(
        &mut system,
        &routing,
        &ActorStatusNotice {
            runtime_id: 42,
            kind: ActorStatusKind::BabyAge,
            data: 0,
            position,
            height: Some(height),
            eating_position: position,
        },
    );
    system.tick(0.001, &EmptyWorld);
    let draw = system.build_draw(
        &ParticleView {
            position: [1.0, 64.0, 12.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        },
        &EmptyWorld,
    );
    assert_eq!(draw.opaque.len(), 15);
    let body = [1.0, 64.0 + height * BODY_HEIGHT_FRACTION, 2.0];
    for particle in &draw.opaque {
        assert_eq!(particle.center_light[..3], body);
    }
}
