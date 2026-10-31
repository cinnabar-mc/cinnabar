use super::*;
use crate::{emitter::SpawnRequest, world::EmptyWorld};

/// Builds matching mixed materials with a render expression that consumes random values.
fn system(count: usize) -> ParticleSystem {
    let mut system = ParticleSystem::default();
    for material in ["particles_alpha", "particles_blend", "particles_add"] {
        let id = format!("test:{material}");
        let effect = tests::EFFECT
            .replace("minecraft:quad", &id)
            .replace("particles_alpha", material)
            .replace("0.5,0.25", "\"math.random(0.2,0.5)\",0.25");
        assert!(system.register_effect(effect.as_bytes()));
        for index in 0..count {
            system.spawn(&SpawnRequest {
                effect: id.clone(),
                position: [0.0, 0.0, -3.0 - index as f32 * 0.01],
                ..Default::default()
            });
        }
    }
    system.tick(0.02, &EmptyWorld);
    system
}

#[test]
fn reused_output_matches_fresh_lists_through_count_and_visibility_changes() {
    let mut fresh = system(32);
    let mut reused = system(32);
    let mut output = DrawLists::default();
    let mut pointers = None;
    for limit in [128.0, 4.0, 128.0, 0.0, 128.0] {
        let hide = |low: [f32; 3], _| low[2] < -limit;
        let expected = fresh.build_draw_with_visibility(&tests::view(), &EmptyWorld, hide);
        reused.build_draw_into_with_visibility(&tests::view(), &EmptyWorld, hide, &mut output);
        for (expected, actual) in [
            (&expected.opaque, &output.opaque),
            (&expected.blend, &output.blend),
            (&expected.add, &output.add),
        ] {
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(expected),
                bytemuck::cast_slice::<_, u8>(actual)
            );
        }
        assert_eq!(expected.cave_hidden, output.cave_hidden);
        assert_eq!(expected.light_lookups, output.light_lookups);
        assert_eq!(expected.light_cache_hits, output.light_cache_hits);
        let current = [
            output.opaque.as_ptr(),
            output.blend.as_ptr(),
            output.add.as_ptr(),
        ];
        assert_eq!(*pointers.get_or_insert(current), current);
    }
}

#[test]
fn retained_particle_output_trims_only_after_sustained_lower_counts() {
    let mut system = system(64);
    let mut output = DrawLists::default();
    system.build_draw_into_with_visibility(&tests::view(), &EmptyWorld, |_, _| false, &mut output);
    let capacities = [
        output.opaque.capacity(),
        output.blend.capacity(),
        output.add.capacity(),
    ];
    for frame in 1..=render_model::CapacityRetention::SHRINK_AFTER {
        system.build_draw_into_with_visibility(
            &tests::view(),
            &EmptyWorld,
            |_, _| true,
            &mut output,
        );
        let actual = [
            output.opaque.capacity(),
            output.blend.capacity(),
            output.add.capacity(),
        ];
        assert_eq!(
            actual,
            if frame == render_model::CapacityRetention::SHRINK_AFTER {
                [0; 3]
            } else {
                capacities
            }
        );
    }
}
