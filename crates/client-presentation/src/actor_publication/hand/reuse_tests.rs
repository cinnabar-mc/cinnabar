use super::*;
use bevy::ecs::system::SystemState;
use bevy::prelude::{PerspectiveProjection, Projection, Time, Transform, World};
use bevy::time::Real;
use native_tests::{fixture_with_clock, fixture_with_visibility};

/// Runs the production readiness and final publication owners frame by frame.
struct Frames {
    world: World,
    stream: WorldStream,
    entities: Arc<assets::RuntimeEntityAssets>,
    input: ActorEquipmentInput,
    params: SystemState<super::super::ActorFramePublication<'static, 'static>>,
}

impl Frames {
    fn new(main: Option<&str>) -> Self {
        Self::with_fixture(fixture_with_clock(main, false))
    }

    /// Runs the production owners over an already built player fixture.
    fn with_fixture(
        ((stream, equipment, artwork, input), entities): (
            (
                WorldStream,
                EquipmentRuntime,
                render_model::ActorArtworkPages,
                ActorEquipmentInput,
            ),
            Arc<assets::RuntimeEntityAssets>,
        ),
    ) -> Self {
        let mut world = World::new();
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(equipment);
        world.insert_resource(render_scene::ActorArtworkResource(artwork));
        world.insert_resource(
            render_scene::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap(),
        );
        world
            .insert_resource(super::super::HandRigBuilder::from_runtime_assets(&entities).unwrap());
        world.init_resource::<super::super::ActorFrameState>();
        world.init_resource::<super::super::PreparedActorPublication>();
        world.init_resource::<super::super::ActorFramePartialTick>();
        world.init_resource::<render_scene::HandRigScene>();
        let mut avatar = crate::local_player::LocalAvatarPresentation::default();
        avatar.begin_session(
            simulation_clock::identity::SessionGeneration::new(
                stream.authority().actor_session_id(),
            ),
            1,
        );
        world.insert_resource(avatar);
        world.init_resource::<crate::local_player::LocalAvatarVisibilityCarrier>();
        let mut settings = crate::camera::CameraSettingsAuthority::default();
        let mut user = ui::UserSettings::default();
        user.gameplay.default_perspective = semantic_input::PerspectiveMode::FirstPerson;
        user.video.java_animations = false;
        settings.replace(1, &user).unwrap();
        world.insert_resource(settings);
        world.insert_resource(crate::local_player::LocalViewPose::default());
        world.spawn((
            Transform::default(),
            Projection::Perspective(PerspectiveProjection::default()),
            crate::camera::FlyCamera::default(),
        ));
        let params = SystemState::new(&mut world);
        Self {
            world,
            stream,
            entities,
            input,
            params,
        }
    }

    /// One rendered frame advancing `millis` with an optional admitted swing.
    fn frame(&mut self, millis: u64, swing: Option<actor_animation::LocalSwingProgress>) {
        self.world
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(millis));
        let mut prepared_artwork = None;
        super::super::advance_actor_frame(
            super::super::ActorWorld {
                stream: Some(&mut self.stream),
                collisions: None,
                entity_assets: Some(&self.entities),
                pack_entities: None,
                session_items: None,
                prepared_actor_artwork: &mut prepared_artwork,
            },
            super::super::ActorFrameInput {
                cosmetics: Vec::new(),
                local_feed: None,
                predicted_eye: Some([0.0, 65.62, 0.0]),
                predicted_feet: Some([0.0, 64.0, 0.0]),
                local_equipment: self.input.clone(),
                swing_progress: None,
                renders_game: true,
                hide_hand: false,
                remote_emotes: Vec::new(),
                remote_hats: Vec::new(),
                custom_emote: None,
            },
            |_| {},
            |_, _| (None, None),
            self.params.get_mut(&mut self.world).unwrap(),
        );
        super::super::prepare_actor_render_frame(
            super::super::ActorWorld {
                stream: Some(&mut self.stream),
                collisions: None,
                entity_assets: Some(&self.entities),
                pack_entities: None,
                session_items: None,
                prepared_actor_artwork: &mut prepared_artwork,
            },
            swing,
            |_, _, _, _| false,
            None,
            self.params.get_mut(&mut self.world).unwrap(),
        );
    }

    fn kept(&self) -> u64 {
        self.world
            .resource::<super::super::ActorFrameState>()
            .hand_published
            .kept
    }

    /// Whether the last frame published a drawable hand.
    fn drawn(&self) -> bool {
        self.world
            .resource::<render_scene::HandRigScene>()
            .is_active()
    }

    fn reuses(&self) -> u64 {
        self.world
            .resource::<super::super::ActorFrameState>()
            .hand_reuse
            .reuses
    }

    /// The published hand's camera-space draws.
    fn published(&self) -> Vec<render_model::ActorGpuInstance> {
        self.world
            .resource::<render_scene::HandRigScene>()
            .frame
            .as_ref()
            .expect("the hand draws")
            .rig
            .instances
            .to_vec()
    }
}

/// An idle empty hand keeps its source across frames whose frame fraction differs, and the
/// debug rebuild inside readiness proves each reuse matches a fresh source.
#[test]
fn idle_empty_hand_reuses_its_source_across_frames() {
    let mut frames = Frames::new(None);
    frames.frame(7, None);
    let first = frames.published();
    for millis in [3, 11, 5] {
        frames.frame(millis, None);
    }
    assert_eq!(
        frames.reuses(),
        3,
        "every idle frame after the first reuses the source"
    );
    assert_eq!(frames.kept(), 3, "the scene keeps the identical idle frame");
    let instance = |instances: Vec<render_model::ActorGpuInstance>| {
        instances
            .into_iter()
            .map(|instance| (instance.world_from_actor, instance.geometry_id))
            .collect::<Vec<_>>()
    };
    assert_eq!(instance(frames.published()), instance(first));
}

/// While the arm swings across ticks its phases move with the frame fraction, so no source is
/// kept; the debug rebuild checks the frames that do reuse one.
#[test]
fn swinging_arm_is_rebuilt_every_frame() {
    let mut frames = Frames::new(None);
    frames.frame(7, None);
    frames.frame(7, None);
    assert_eq!(frames.reuses(), 1);
    let swing = |previous, current| {
        Some(actor_animation::LocalSwingProgress {
            bedrock: [previous, current],
            java: [previous, current],
            frame_alpha: Some(0.5),
        })
    };
    frames.frame(60, swing(0.0, 0.25));
    frames.frame(60, swing(0.25, 0.5));
    let kept = frames.kept();
    frames.frame(60, swing(0.5, 0.75));
    assert_eq!(
        frames.reuses(),
        1,
        "a moving swing reads the frame fraction"
    );
    assert_eq!(frames.kept(), kept, "a moving swing republishes");
}

/// A held item's source samples the native parent pose at the frame fraction and is rebuilt.
#[test]
fn held_items_are_rebuilt_every_frame() {
    let mut frames = Frames::new(Some("minecraft:shield"));
    for millis in [7, 3, 11] {
        frames.frame(millis, None);
    }
    assert_eq!(frames.reuses(), 0);
}

/// An empty hand whose render controller is drawn only past a camera pitch threshold hides on
/// the first frame after the camera crosses back, within one actor tick and with unchanged actor
/// and rig revisions, instead of keeping the earlier source.
#[test]
fn camera_gated_controller_rebuilds_when_the_pitch_crosses_its_threshold() {
    let mut frames = Frames::with_fixture(fixture_with_visibility(
        None,
        false,
        false,
        true,
        false,
        false,
        None,
        Some("math.abs(query.camera_rotation(0)) > 10.0"),
    ));
    let look = |frames: &mut Frames, degrees: f32| {
        frames
            .world
            .insert_resource(crate::local_player::LocalViewPose::new(
                bevy::math::Vec3::new(0.0, 65.62, 0.0),
                bevy::math::Quat::from_rotation_x(-degrees.to_radians()),
            ));
    };
    look(&mut frames, 20.0);
    for millis in [7, 60, 3, 3] {
        frames.frame(millis, None);
    }
    assert!(frames.drawn(), "a lowered camera draws the gated arm");
    let revision = frames.stream.authority().actor_presentation_revision(1);
    look(&mut frames, 0.0);
    frames.frame(3, None);
    assert_eq!(
        frames.stream.authority().actor_presentation_revision(1),
        revision,
        "the turn stays within one actor tick"
    );
    assert!(
        !frames.drawn(),
        "the first frame past the threshold hides the arm"
    );
}

/// A scene cleared by another owner is never mistaken for the kept frame: the next identical
/// frame republishes and the hand draws again.
#[test]
fn identical_frame_after_an_outside_clear_restores_the_hand() {
    let mut frames = Frames::new(None);
    frames.frame(7, None);
    frames.frame(7, None);
    assert!(frames.drawn());
    let kept = frames.kept();
    frames
        .world
        .resource_mut::<render_scene::HandRigScene>()
        .clear();
    frames.frame(7, None);
    assert!(
        frames.drawn(),
        "the identical frame must be published again"
    );
    assert_eq!(frames.kept(), kept, "a cleared scene holds nothing to keep");
    frames.frame(7, None);
    assert_eq!(frames.kept(), kept + 1, "the restored frame is kept again");
}

/// A frame another owner publishes under the recorded revision gets its own token from the
/// scene, so the hand republishes instead of keeping a frame it did not build.
#[test]
fn outside_publication_under_the_same_revision_is_not_kept() {
    let mut frames = Frames::new(None);
    frames.frame(7, None);
    frames.frame(7, None);
    let kept = frames.kept();
    let held = frames
        .world
        .resource::<render_scene::HandRigScene>()
        .frame
        .clone()
        .expect("the hand draws");
    let mut scene = frames.world.resource_mut::<render_scene::HandRigScene>();
    assert!(scene.publish(
        held.rig,
        held.skin,
        held.light,
        held.fov_radians,
        held.revision
    ));
    frames.frame(7, None);
    assert_eq!(frames.kept(), kept, "an outside frame is republished over");
    frames.frame(7, None);
    assert_eq!(frames.kept(), kept + 1);
}

/// Sets the local actor's mark variant as a server metadata update between ticks.
fn mark(frames: &mut Frames, value: i32) {
    let sequence = frames.stream.authority().actor_sequence_frontier() + 1;
    frames
        .stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: [protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::MarkVariant.wire_value(),
                        value: protocol::ActorMetadataValue::Int(value),
                    }]
                    .into(),
                    properties: [].into(),
                    tick: 0,
                },
            )),
        )
        .unwrap();
}

/// A metadata change between ticks changes the actor's revision, so the next frame samples
/// again even at a repeated frame fraction and camera instead of reusing the earlier sample, and
/// no later frame keeps a source built from it.
#[test]
fn metadata_between_ticks_resamples_at_a_repeated_fraction() {
    let mut frames = Frames::with_fixture(fixture_with_visibility(
        None,
        false,
        false,
        true,
        true,
        false,
        None,
        Some("query.mark_variant == 1"),
    ));
    mark(&mut frames, 1);
    for millis in [7, 60, 3, 3] {
        frames.frame(millis, None);
    }
    let samples = |frames: &Frames| {
        frames
            .world
            .resource::<super::super::ActorFrameState>()
            .java_hand
            .native_pose
            .layer_samples
    };
    frames.frame(0, None);
    let repeated = samples(&frames);
    frames.frame(0, None);
    assert_eq!(
        samples(&frames),
        repeated,
        "an unchanged frame reuses its sample"
    );
    let revision = frames.stream.authority().actor_presentation_revision(1);
    mark(&mut frames, 0);
    assert_ne!(
        frames.stream.authority().actor_presentation_revision(1),
        revision,
        "metadata changes the actor's revision"
    );
    frames.frame(0, None);
    assert!(
        samples(&frames) > repeated,
        "the same fraction and camera sample the changed actor again"
    );
    // The debug rebuild inside readiness checks any source kept from here on.
    frames.frame(3, None);
}

/// Editing the held frame's artwork in place gives it a new publication token, so the hand
/// republishes instead of keeping the edited frame.
#[test]
fn artwork_edited_in_place_is_republished() {
    let mut frames = Frames::new(None);
    frames.frame(7, None);
    frames.frame(7, None);
    let kept = frames.kept();
    let mut scene = frames.world.resource_mut::<render_scene::HandRigScene>();
    let before = scene.publication();
    scene.set_map_background(None);
    assert_ne!(scene.publication(), before, "an edit advances the token");
    frames.frame(7, None);
    assert_eq!(frames.kept(), kept, "the edited frame is republished");
    assert!(
        frames
            .world
            .resource::<render_scene::HandRigScene>()
            .frame
            .as_ref()
            .is_some_and(|frame| frame.item_atlases[2].is_some()),
        "republishing restores the map background"
    );
}
