use bevy::prelude::Vec3;
use client_world::LoyaltyRopeView;
use render_model::{BlockEntityKind, BlockEntityLight, BlockEntitySubmission, LoyaltyRopeModel};

/// Admits ropes by their trident's interpolated position, lit like the trident they leave.
pub(super) fn submit(
    submissions: &mut Vec<BlockEntitySubmission>,
    ropes: impl IntoIterator<Item = LoyaltyRopeView>,
    camera: Option<Vec3>,
    mut light_at: impl FnMut(u64, [f32; 3]) -> Option<(u8, u8)>,
) {
    for rope in ropes {
        if camera.is_some_and(|camera| {
            !client_presentation::presentation::actors::within_actor_candidate_cube(
                rope.trident,
                camera.to_array(),
            )
        }) {
            continue;
        }
        if submissions.len() >= super::MAX_SUBMISSIONS {
            break;
        }
        submissions.push(BlockEntitySubmission {
            block: rope.trident.map(|value| value.floor() as i32),
            light: light_at(rope.runtime_id, rope.trident).map_or_else(
                || 1.0.into(),
                |(block, sky)| BlockEntityLight::Actor { block, sky },
            ),
            kind: BlockEntityKind::LoyaltyRope(LoyaltyRopeModel {
                trident: rope.trident,
                span: rope.span,
                owner_yaw_degrees: rope.owner_yaw_degrees,
                phase: rope.phase,
                return_ticks: rope.return_ticks,
            }),
        });
    }
}
