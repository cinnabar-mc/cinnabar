use super::*;

/// A system with minimal authored fishing effects, so emitter counts reflect routing alone.
fn system() -> ParticleSystem {
    let mut system = ParticleSystem::default();
    for effect in [
        assets::facts::ParticleEffect::FishHookParticle,
        assets::facts::ParticleEffect::FishPosParticle,
        assets::facts::ParticleEffect::WaterWakeParticle,
        assets::facts::ParticleEffect::WaterSplashParticle,
    ] {
        let json = format!(
            r#"{{"particle_effect":{{"description":{{"identifier":"{}","basic_render_parameters":{{"material":"particles_alpha","texture":"test"}}}},"components":{{"minecraft:emitter_rate_instant":{{"num_particles":1}},"minecraft:emitter_lifetime_once":{{}},"minecraft:emitter_shape_point":{{}},"minecraft:particle_lifetime_expression":{{"max_lifetime":1}},"minecraft:particle_appearance_billboard":{{"size":[0.05,0.05]}}}}}}}}"#,
            effect.identifier()
        );
        assert!(system.register_effect(json.as_bytes()));
    }
    system
}

/// Routes one fishing event through the production notice boundary; returns emitters started.
fn route(kind: ActorStatusKind, fishing: Option<client_world::FishingHookPose>) -> usize {
    let stream = super::totem_tests::stream();
    let registry = sim::CollisionRegistry::default();
    let world = StreamParticleWorld::new(&stream, &registry, None);
    let routing = Routing {
        world: &world,
        stream: &stream,
        mode: NetworkIdMode::Sequential,
        icons: None,
    };
    let mut system = system();
    route_notice(
        &mut system,
        &routing,
        &ActorStatusNotice {
            runtime_id: 42,
            kind,
            data: 0,
            position: [0.5, 0.75, 0.5],
            height: Some(0.25),
            eating_position: [0.5, 0.75, 0.5],
            fishing,
        },
    );
    system.emitter_count()
}

#[test]
fn hook_events_draw_their_native_effects() {
    let hook = client_world::FishingHookPose {
        identifier: "minecraft:fishing_hook".into(),
        bottom: 0.75,
        width: 0.25,
        fish_offset: [1.0, 1.0],
        fish_angle: 0.5,
        silent: false,
    };
    assert_eq!(route(ActorStatusKind::FishHookBite, Some(hook.clone())), 3);
    let trail = route(ActorStatusKind::FishHookPosition, Some(hook.clone()));
    assert!((2..=3).contains(&trail), "{trail} trail emitters");
    // A tease only splashes where the fish's cell holds water; this world has none.
    assert_eq!(route(ActorStatusKind::FishHookTease, Some(hook)), 0);
    for kind in [
        ActorStatusKind::FishHookBite,
        ActorStatusKind::FishHookPosition,
        ActorStatusKind::FishHookTease,
    ] {
        assert_eq!(route(kind, None), 0, "{kind:?} on an actor that is no hook");
    }
}
