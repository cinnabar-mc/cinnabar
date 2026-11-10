//! Initial actor snapshots from network spawns and local player state.

use super::*;

impl ActorSnapshot {
    /// Builds retained state using the constructor facts resolved at admission.
    pub(super) fn from_spawn(
        spawn: ActorSpawnEvent,
        spawn_revision: u64,
        render_spawn_revision: NonZeroU64,
        entity_fact: EntityFact,
        hitbox_budget: &mut hitbox::Budget,
    ) -> Self {
        let terrain_interlock = terrain_interlock::TerrainInterlock::attached(
            entity_fact.native_class,
            web_time::Instant::now(),
        );
        let position = spawn.position;
        let pitch = movement_interpolation::signed_pitch(spawn.pitch);
        let pose = ActorPose {
            y_origin: PoseYOrigin {
                native_y: (entity_fact.native_class == NativeEntityClass::Minecart)
                    .then_some(position[1]),
                ..PoseYOrigin::SPAWN
            },
            position,
            pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
        };
        let mut snapshot = Self {
            painting: None,
            unique_id: spawn.unique_id,
            runtime_id: spawn.runtime_id,
            spawn_revision,
            render_spawn_revision,
            movement_revision: 0,
            metadata_revision: 0,
            interaction_traits: assets::interactions::EntityInteractionTraits::Unresolved,
            kind: spawn.kind,
            entity_fact,
            position,
            velocity: spawn.velocity,
            pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: spawn.body_yaw,
            on_ground: None,
            teleported: false,
            player_mode: None,
            player_game_mode: None,
            source_tick: None,
            metadata: HashMap::with_capacity(spawn.metadata.len()),
            explicit_dimensions: ExplicitDimensions::for_spawn(spawn_revision),
            local_presentation_flags: None,
            attributes: HashMap::with_capacity(spawn.attributes.len()),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: ActorStatus {
                native_velocity: spawn.velocity,
                terrain_interlock,
                ..ActorStatus::default()
            },
            dragon_animation: None,
            hitboxes: None,
        };
        snapshot.set_current_pose(pose);
        // Falling-block, boat and minecart spawns carry their network offset, as movement does;
        // a falling block's box sets it and `apply_metadata` rebases onto the spawn's box.
        let spawn_offset = snapshot.spawn_network_offset();
        if spawn_offset != 0.0 {
            snapshot.shift_poses_y(-spawn_offset, -spawn_offset);
        }
        snapshot.apply_metadata(&spawn.metadata, spawn_revision, hitbox_budget);
        snapshot.apply_attributes(&spawn.attributes);
        snapshot.sync_status_from_health();
        snapshot.apply_properties(&spawn.properties);
        snapshot
    }

    /// Creates a synthetic actor in arrival order, with a separate presentation revision.
    /// The resolved player identity selects its skin.
    pub(super) fn local_player(
        unique_id: i64,
        runtime_id: u64,
        (spawn_revision, movement_revision, render_spawn_revision): (u64, u64, NonZeroU64),
        uuid: [u8; 16],
        username: std::sync::Arc<str>,
        feed: &LocalPlayerFeed,
        authority: Option<Self>,
    ) -> Self {
        let pose = ActorPose {
            y_origin: PoseYOrigin::default(),
            position: feed.position,
            pitch: feed.pitch,
            yaw: feed.yaw,
            head_yaw: feed.head_yaw,
        };
        let mut snapshot = authority.unwrap_or_else(|| Self {
            painting: None,
            unique_id,
            runtime_id,
            spawn_revision,
            render_spawn_revision,
            movement_revision,
            metadata_revision: 0,
            interaction_traits: assets::interactions::EntityInteractionTraits::GENERIC_MOB,
            kind: ActorKind::Player {
                uuid,
                username: username.clone(),
            },
            entity_fact: EntityFacts::pinned().player(),
            position: feed.position,
            velocity: feed.velocity,
            pitch: feed.pitch,
            yaw: feed.yaw,
            head_yaw: feed.head_yaw,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: feed.yaw,
            on_ground: Some(feed.on_ground),
            teleported: feed.teleported,
            player_mode: None,
            player_game_mode: None,
            source_tick: None,
            metadata: HashMap::new(),
            explicit_dimensions: ExplicitDimensions::for_spawn(spawn_revision),
            local_presentation_flags: None,
            attributes: HashMap::new(),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: ActorStatus {
                native_velocity: feed.velocity,
                fall_fly_ticks: feed.fall_fly_ticks,
                ..ActorStatus::default()
            },
            dragon_animation: None,
            hitboxes: None,
        });
        snapshot.unique_id = unique_id;
        snapshot.runtime_id = runtime_id;
        snapshot.spawn_revision = spawn_revision;
        snapshot.render_spawn_revision = render_spawn_revision;
        snapshot.explicit_dimensions.spawn_revision = Some(spawn_revision);
        snapshot.movement_revision = movement_revision;
        snapshot.kind = ActorKind::Player { uuid, username };
        snapshot.previous_pose = pose;
        snapshot.received_pose = pose;
        snapshot.set_current_pose(pose);
        snapshot.body_yaw = feed.yaw;
        snapshot.interpolation_ticks_remaining = 0;
        snapshot.velocity = feed.velocity;
        snapshot.status.native_velocity = feed.velocity;
        snapshot.status.fall_fly_ticks = feed.fall_fly_ticks;
        snapshot.on_ground = Some(feed.on_ground);
        snapshot.teleported = feed.teleported;
        // Retain the visible-name default until the server replaces it.
        snapshot
            .metadata
            .entry(ActorDataKey::Flags.wire_value())
            .or_insert(ActorMetadataValue::Flags(
                (1 << ActorFlag::ShowName.wire_value())
                    | (1 << ActorFlag::HasGravity.wire_value())
                    | (1 << ActorFlag::HasCollision.wire_value()),
            ));
        snapshot.apply_local_flags(feed);
        snapshot
    }
}
