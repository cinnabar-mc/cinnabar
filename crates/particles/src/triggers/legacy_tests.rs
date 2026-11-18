use super::*;

/// The active Bedrock wire selectors distinguish colored dust from the three spell variants.
#[test]
fn current_wire_ids_keep_colored_dust_and_spells_distinct() {
    let packed = 0xff22_aa55_u32 as i32;
    let color = [34.0 / 255.0, 170.0 / 255.0, 85.0 / 255.0, 1.0];
    for (id, effect) in [
        (33, ParticleEffect::FallingDust),
        (34, ParticleEffect::MobspellEmitter),
        (35, ParticleEffect::MobspellEmitter),
        (36, ParticleEffect::MobspellEmitter),
    ] {
        assert_eq!(
            classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | id, packed),
            Some(LevelParticle::Named {
                effect,
                spell_color: Some(color)
            }),
            "wire particle {id}"
        );
    }
}

#[test]
fn current_item_and_terrain_selectors_keep_their_payloads() {
    for id in [14, 52] {
        assert_eq!(
            classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | id, (300 << 16) | 7),
            Some(LevelParticle::ItemIcon {
                network_id: 300,
                aux: 7
            })
        );
    }
    assert_eq!(
        classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | 21, 0xfabc_def0_u32 as i32),
        Some(LevelParticle::Terrain {
            runtime_id: 0xfabc_def0_u32 as i32
        })
    );
    assert_eq!(
        classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | 38, 0),
        Some(LevelParticle::FixedItemIcon {
            identifier: assets::facts::VanillaItem::SlimeBall.identifier(),
            count: ITEM_ICON_PARTICLES,
        })
    );
}

#[test]
fn native_id_insertion_boundaries_and_reordered_types_select_their_effects() {
    for (id, effect) in [
        (9, ParticleEffect::CandleFlameParticle),
        (10, ParticleEffect::LavaParticle),
        (11, ParticleEffect::BasicSmokeParticle),
        (12, ParticleEffect::RedstoneTorchDustParticle),
        (13, ParticleEffect::RisingBorderDustParticle),
        (18, ParticleEffect::BreezeWindExplosionEmitter),
        (19, ParticleEffect::MobflameSingle),
        (20, ParticleEffect::HeartParticle),
        (22, ParticleEffect::MyceliumDustParticle),
        (23, ParticleEffect::BasicPortalParticle),
        (24, ParticleEffect::MobPortal),
        (28, ParticleEffect::WaterDripParticle),
        (29, ParticleEffect::LavaDripParticle),
        (30, ParticleEffect::HoneyDripParticle),
        (31, ParticleEffect::StalactiteWaterDripParticle),
        (32, ParticleEffect::StalactiteLavaDripParticle),
        (37, ParticleEffect::InkEmitter),
        (39, ParticleEffect::RainSplashParticle),
        (51, ParticleEffect::TotemManual),
        (62, ParticleEffect::Sneeze),
        (83, ParticleEffect::ShriekParticle),
        (90, ParticleEffect::VaultConnectionParticle),
        (91, ParticleEffect::WindExplosionEmitter),
        (98, ParticleEffect::GreenFlameParticle),
        (101, ParticleEffect::SulfurCubeGoo),
    ] {
        assert_eq!(
            classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | id, 0),
            Some(LevelParticle::Named {
                effect,
                spell_color: None
            }),
            "wire particle {id}"
        );
    }
    assert!(classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | 0xfff, 0).is_none());
}

#[test]
fn colored_level_requests_reach_the_draw_list_at_the_server_position() {
    use crate::{EmptyWorld, ParticleSystem, ParticleView};
    for id in [33, 34, 35, 36] {
        let Some(LevelParticle::Named {
            effect,
            spell_color,
        }) = classify_level_event(LEVEL_EVENT_PARTICLE_FLAG | id, 0xff22_aa55_u32 as i32)
        else {
            panic!("colored native selector must resolve a named effect");
        };
        let document = serde_json::json!({"particle_effect": {
            "description": {"identifier": effect.identifier(), "basic_render_parameters": {
                "material": "particles_alpha", "texture": "original_fixture"}},
            "components": {
                "minecraft:emitter_rate_instant": {"num_particles": 1},
                "minecraft:emitter_lifetime_once": {},
                "minecraft:emitter_shape_point": {},
                "minecraft:particle_lifetime_expression": {"max_lifetime": 1},
                "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]},
                "minecraft:particle_appearance_tinting": {
                    "color": ["v.color.r", "v.color.g", "v.color.b", "v.color.a"]}
            }
        }});
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(&serde_json::to_vec(&document).unwrap()));
        let position = [0.0, 0.1, 0.0];
        system
            .spawn(&native_request(effect, position, spell_color))
            .unwrap();
        system.tick(0.01, &EmptyWorld);
        let draw = system.build_draw(
            &ParticleView {
                position: [0.0, 0.1, 3.0],
                right: [1.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                forward: [0.0, 0.0, -1.0],
                half_diagonal: 1.0,
            },
            &EmptyWorld,
        );
        let [particle] = draw.opaque.as_slice() else {
            panic!("one colored particle must draw");
        };
        assert_eq!(particle.color, spell_color.unwrap());
        assert_eq!(particle.center_light[..3], position);
    }
}
