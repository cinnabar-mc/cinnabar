//! Block picks are trimmed from the ticked network position, which sits at eye height.

use super::*;

#[test]
fn block_pick_trim_measures_from_eye_height_above_the_feet() {
    let feet = Vec3::new(0.5, 64.0, 0.5);
    let reach = crate::mining::survival_reach(PlayerInputMode::Mouse);
    // Centre (2.5, 70.5, 2.5): 5.64 from the eye, 7.09 from the feet.
    let block = [2, 70, 2];
    // A horizontal pose moves only the camera eye, never the network position.
    for swim_pose_active in [false, true] {
        let mut physics = LocalPhysicsController::default();
        let mut state = PlayerState::new(feet);
        state.swim_pose_active = swim_pose_active;
        physics.state = Some(state);
        let position = physics.network_position().unwrap();
        assert_eq!(position, [0.5, 64.0 + PLAYER_NETWORK_OFFSET, 0.5]);
        assert!(crate::interaction_authority::block_pick_in_range(
            block, position, reach
        ));
        assert!(!crate::interaction_authority::block_pick_in_range(
            block,
            [0.5, 64.0, 0.5],
            reach
        ));
    }
}
