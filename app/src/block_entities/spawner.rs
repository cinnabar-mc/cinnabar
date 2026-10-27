//! Spawner contents use the actor evaluator, artwork pages and draw publication.
use crate::runtime::world::ClientWorld;
use bevy::platform::collections::HashMap;
use bevy::prelude::{Mat4, Projection, Query, Res, ResMut, Resource, Transform, Vec3, With};
use client_presentation::{
    actor_publication::{ActorFrameState, PreparedActorPublication},
    camera::FlyCamera,
    local_player::LocalViewPose,
    presentation::{
        actors::{ActorPresentationBatch, PoseConversions, entity_rig_presentation_cached},
        entity_layers::{LayerPoseCache, apply_render_layers_cached},
    },
};
use client_world::DisplayActorStore;
use render_model::spawner::{SpawnerDisplayClock, spawner_actor_transform};
use render_model::{ActorArtworkPages, ActorCullView, matrix_rows};
use std::sync::{Arc, Weak};
use world::{BlockEntityKey, BlockEntityNbt, Chunk, ChunkKey, SUB_CHUNK_SIDE};

use super::system::{SCAN_RADIUS_BLOCKS as DISPLAY_SCAN_RADIUS, columns};
const DEFAULT_ACTIVATION_RANGE: f32 = 16.0;
const DEFAULT_SPAWN_DELAY: i32 = 0;

type EntityIndex = std::collections::BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>;
type Candidate = (ChunkKey, usize);
/// A resolved lifetime and its frame's placement inputs.
type Display = (u64, [i32; 3], SpawnerDisplayClock);

/// Temporary request order retained by one display publisher.
#[derive(Default)]
struct PreparationScratch {
    candidates: Vec<Candidate>,
    offscreen: Vec<Candidate>,
    positions: Vec<[i32; 3]>,
    players: Vec<Vec3>,
    displays: Vec<Display>,
}

impl PreparationScratch {
    /// Filters requests and puts in-view candidates first without changing either group's scan order.
    fn prepare_candidates(
        &mut self,
        requests: impl Iterator<Item = Candidate>,
        mut in_range: impl FnMut(Candidate) -> bool,
        mut in_view: impl FnMut(Candidate) -> bool,
    ) {
        self.candidates.clear();
        self.offscreen.clear();
        for request in requests {
            if !in_range(request) {
                continue;
            }
            if in_view(request) {
                self.candidates.push(request);
            } else {
                self.offscreen.push(request);
            }
        }
        self.candidates.append(&mut self.offscreen);
    }
}

struct Column {
    source: Weak<EntityIndex>,
    displays: Vec<Request>,
}
impl Column {
    /// Whether the column still owns the index used by this scan.
    fn is_current(&self, chunk: &Chunk) -> bool {
        chunk.holds_block_entities(&self.source)
    }
}

/// Everything a column pass reads besides the pool's own scans.
#[derive(Clone, PartialEq, Eq)]
struct ListingInputs {
    window: columns::ColumnWindow,
    mode: assets::NetworkIdMode,
    registry: [u8; 32],
}

/// Spawner requests on spawner blocks, as `(column, display index)` in scan order.
struct Listing {
    /// Present only when the pass left every column in range current, so an identical pass
    /// would rescan nothing and list the same requests.
    settled: Option<ListingInputs>,
    requests: Vec<(ChunkKey, usize)>,
}
#[derive(Clone)]
struct Request {
    position: [i32; 3],
    identifier: Arc<str>,
    delay: i32,
    activation_range: f32,
    nbt: Arc<BlockEntityNbt>,
    /// A rejection remains valid while this snapshot and the active pack are unchanged.
    rigless: bool,
}
struct Clock {
    source: Arc<BlockEntityNbt>,
    spin: SpawnerDisplayClock,
}
struct Pool {
    source: Arc<assets::RuntimeEntityAssets>,
    session: (u64, i32),
    pack: Option<Arc<assets::SessionEntityPack>>,
    actors: DisplayActorStore,
    columns: HashMap<ChunkKey, Column>,
    rescans: columns::RescanOrder,
    spins: HashMap<[i32; 3], Clock>,
    poses: PoseConversions,
    layers: LayerPoseCache,
    /// The last column pass, reused while its inputs and the terrain are unchanged.
    listing: Option<Listing>,
    scratch: PreparationScratch,
}

#[derive(Resource, Default)]
pub(crate) struct SpawnerDisplayPublisher(Option<Pool>);

#[cfg(feature = "developer-control")]
impl SpawnerDisplayPublisher {
    /// Returns the display actor published under this lifetime, or `None` for another namespace.
    pub(crate) fn actor(
        &self,
        session_id: u64,
        runtime_id: u64,
    ) -> Option<&client_world::ActorSnapshot> {
        let pool = self.0.as_ref()?;
        (pool.actors.session_id() == session_id)
            .then(|| pool.actors.actor(runtime_id))
            .flatten()
    }
}

/// Parses the spawners of a column NBT index, reusing requests whose snapshot is unchanged.
fn column(source: Arc<EntityIndex>, previous: Option<&Column>) -> Column {
    let previous = previous.map_or(&[][..], |column| column.displays.as_slice());
    let displays = source
        .iter()
        .filter(|(_, nbt)| {
            nbt.id()
                .and_then(|id| assets::facts::BlockEntityFacts::pinned().entry(id))
                .is_some_and(|entry| {
                    entry.renderer == assets::facts::BlockEntityRenderer::MobSpawner
                })
        })
        .filter_map(|(key, nbt)| {
            let position = key.position();
            // Requests follow the index's key order, so the previous column is sorted by position.
            if let Ok(index) = previous.binary_search_by(|request| request.position.cmp(&position))
                && Arc::ptr_eq(&previous[index].nbt, nbt)
            {
                return Some(previous[index].clone());
            }
            let root = nbt.parse()?;
            let identifier = root.string("EntityIdentifier")?;
            if !DisplayActorStore::admits_identifier(identifier) {
                return None;
            }
            Some(Request {
                position,
                identifier: Arc::from(identifier),
                delay: root
                    .integer("Delay")
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or(DEFAULT_SPAWN_DELAY)
                    .max(-1),
                activation_range: root
                    .integer("RequiredPlayerRange")
                    .map_or(DEFAULT_ACTIVATION_RANGE, |value| value.clamp(0, 128) as f32),
                nbt: Arc::clone(nbt),
                rigless: false,
            })
        })
        .collect();
    Column {
        source: Arc::downgrade(&source),
        displays,
    }
}

/// Refreshes current, near or budgeted column scans in range, drops scans that left it, and
/// lists the requests that sit on a spawner block.
fn list_spawners(
    pool: &mut Pool,
    store: &world::ChunkStore,
    dimension: i32,
    inputs: ListingInputs,
) -> Listing {
    let [xs, zs] = inputs.window.columns.clone();
    let eye_column = inputs.window.eye_column;
    let scans = &pool.columns;
    let selected =
        pool.rescans
            .select(inputs.window.stale_distant(dimension, store, |key, chunk| {
                scans.get(&key).is_none_or(|scan| !scan.is_current(chunk))
            }));
    let side = SUB_CHUNK_SIDE as i32;
    let mut rescans_left = columns::MAX_COLUMN_RESCANS_PER_FRAME;
    let mut settled = true;
    let mut requests = Vec::new();
    for x in xs.clone() {
        for z in zs.clone() {
            let key = ChunkKey::new(dimension, x, z);
            let Some(chunk) = store.chunk(key) else {
                continue;
            };
            let near = columns::is_near_column(eye_column, x, z);
            let mut deferred = 0;
            let budget = if near || selected.contains(&Some(key)) {
                &mut rescans_left
            } else {
                &mut deferred
            };
            let Some(scan) = columns::frame_scan(
                pool.columns.remove(&key),
                chunk,
                near,
                budget,
                Column::is_current,
                |previous| column(chunk.shared_block_entities(), previous.as_ref()),
            ) else {
                settled = false;
                continue;
            };
            settled &= scan.is_current(chunk);
            for (index, request) in scan.displays.iter().enumerate() {
                let [x, y, z] = request.position;
                let backing = chunk
                    .sub_chunk(y.div_euclid(side))
                    .and_then(|part| {
                        part.runtime_id(
                            0,
                            x.rem_euclid(side) as u8,
                            y.rem_euclid(side) as u8,
                            z.rem_euclid(side) as u8,
                        )
                    })
                    .and_then(|id| {
                        assets::facts::BlockEntityFacts::pinned().for_registry(
                            id,
                            inputs.mode,
                            inputs.registry,
                        )
                    });
                if backing.is_some_and(|fact| {
                    fact.entity.renderer == assets::facts::BlockEntityRenderer::MobSpawner
                }) {
                    requests.push((key, index));
                }
            }
            pool.columns.insert(key, scan);
        }
    }
    pool.columns.retain(|key, _| {
        key.dimension == dimension
            && xs.contains(&key.x)
            && zs.contains(&key.z)
            && store.chunk(*key).is_some()
    });
    Listing {
        settled: settled.then_some(inputs),
        requests,
    }
}

/// Publishes client-owned display actors into this frame's normal actor draw batch.
pub(crate) fn prepare_spawner_actors(
    world: Res<ClientWorld>,
    view: Res<LocalViewPose>,
    collisions: Res<client_interaction::movement::PhysicsCollisionRegistries>,
    (artwork, state): (
        Res<render_scene::ActorArtworkResource>,
        Res<ActorFrameState>,
    ),
    mut pending: ResMut<PreparedActorPublication>,
    mut publisher: ResMut<SpawnerDisplayPublisher>,
    camera: Query<(&Transform, &Projection), With<FlyCamera>>,
) {
    let (Some(stream), Some(assets)) = (world.stream.as_ref(), world.entity_assets.as_ref()) else {
        publisher.0 = None;
        return;
    };
    let session = (
        stream.authority().actor_session_id(),
        stream.current_dimension(),
    );
    if publisher
        .0
        .as_ref()
        .is_none_or(|pool| pool.session != session || !Arc::ptr_eq(&pool.source, assets))
    {
        publisher.0 = Some(Pool {
            source: Arc::clone(assets),
            session,
            pack: None,
            actors: DisplayActorStore::new(session.1, Arc::clone(assets)),
            columns: Default::default(),
            rescans: Default::default(),
            spins: Default::default(),
            poses: Default::default(),
            layers: Default::default(),
            listing: None,
            scratch: PreparationScratch::default(),
        });
    }
    let pool = publisher.0.as_mut().unwrap();
    let pack_changed = match (&pool.pack, &world.pack_entities) {
        (None, None) => false,
        (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
        _ => true,
    };
    if pack_changed {
        pool.actors
            .set_pack(world.pack_entities.as_ref().map(|pack| {
                (
                    Arc::clone(&pack.assets),
                    pack.bindings
                        .iter()
                        .map(|binding| binding.geometry_candidate)
                        .collect(),
                )
            }));
        pool.pack = world.pack_entities.clone();
        for column in pool.columns.values_mut() {
            for request in &mut column.displays {
                request.rigless = false;
            }
        }
        pool.poses = Default::default();
        pool.layers = Default::default();
    }
    let Some(step) = state.frame_step() else {
        return;
    };
    if let Some((rotation, position)) = state.sampling_camera() {
        pool.actors.set_camera(rotation, position);
    }
    let (eye, feet) = (view.eye_translation(), view.feet_translation());
    let range = |center: f32| {
        ((center - DISPLAY_SCAN_RADIUS) / SUB_CHUNK_SIDE as f32).floor() as i32
            ..=((center + DISPLAY_SCAN_RADIUS) / SUB_CHUNK_SIDE as f32).floor() as i32
    };
    let inputs = ListingInputs {
        window: columns::ColumnWindow {
            terrain: stream.collision_store().content_revision(),
            columns: [range(eye.x), range(eye.z)],
            eye_column: [eye.x, eye.z].map(|axis| (axis / SUB_CHUNK_SIDE as f32).floor() as i32),
        },
        mode: stream.network_id_mode(),
        registry: collisions.breg_sha256(),
    };
    let listing = match pool.listing.take() {
        Some(listing) if listing.settled.as_ref() == Some(&inputs) => listing,
        _ => list_spawners(pool, stream.collision_store(), session.1, inputs),
    };
    let mut scratch = std::mem::take(&mut pool.scratch);
    let view = camera
        .single()
        .ok()
        .map(|(transform, projection)| ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: DISPLAY_SCAN_RADIUS * 2.0,
        });
    scratch.prepare_candidates(
        listing.requests.iter().copied(),
        |(key, index)| {
            display_center(pool.columns[&key].displays[index].position).distance_squared(eye)
                <= DISPLAY_SCAN_RADIUS * DISPLAY_SCAN_RADIUS
        },
        |(key, index)| block_in_view(view, pool.columns[&key].displays[index].position),
    );
    pool.listing = Some(listing);
    // Retire every lifetime outside the wanted set first so in-view newcomers find free slots.
    scratch.positions.clear();
    scratch.positions.extend(
        scratch
            .candidates
            .iter()
            .map(|&(key, index)| &pool.columns[&key].displays[index])
            .filter(|request| !request.rigless)
            .take(client_world::MAX_DISPLAY_ACTORS)
            .map(|request| request.position),
    );
    scratch.positions.sort_unstable();
    pool.actors
        .retain(|position| scratch.positions.binary_search(&position).is_ok());
    scratch.players.clear();
    if !scratch.candidates.is_empty() {
        let authority = stream.authority();
        let local = authority.actor(authority.local_player_runtime_id());
        scratch
            .players
            .extend(authority.present_remote_players().filter_map(|actor| {
                actor
                    .interpolated_position(step.partial_tick)
                    .map(Vec3::from_array)
            }));
        if local.is_some_and(|actor| authority.camera_aim_assist_eligible(actor) == Some(true)) {
            scratch.players.push(feet);
        }
    }
    scratch.positions.clear();
    scratch.displays.clear();
    for &(key, index) in &scratch.candidates {
        if scratch.displays.len() >= client_world::MAX_DISPLAY_ACTORS {
            break;
        }
        let request = &mut pool.columns.get_mut(&key).unwrap().displays[index];
        if request.rigless {
            continue;
        }
        // Only admitted displays count; rejected or unrigged entities leave the budget free.
        let Some(runtime) = pool.actors.resolve(request.position, &request.identifier) else {
            request.rigless = true;
            continue;
        };
        scratch.positions.push(request.position);
        let clock = pool.spins.entry(request.position).or_insert_with(|| Clock {
            source: Arc::clone(&request.nbt),
            spin: SpawnerDisplayClock {
                delay: request.delay,
                ..Default::default()
            },
        });
        if !Arc::ptr_eq(&clock.source, &request.nbt) {
            clock.source = Arc::clone(&request.nbt);
            clock.spin.delay = request.delay;
        }
        let center = display_center(request.position);
        clock.spin.advance(
            step.ticks,
            scratch.players.iter().any(|player| {
                center.distance_squared(*player)
                    <= request.activation_range * request.activation_range
            }),
        );
        scratch
            .displays
            .push((runtime, request.position, clock.spin));
    }
    scratch.positions.sort_unstable();
    pool.spins
        .retain(|position, _| scratch.positions.binary_search(position).is_ok());
    pool.actors
        .retain(|position| scratch.positions.binary_search(&position).is_ok());
    pool.actors.advance(step.ticks);
    let artwork = state.actor_artwork(&artwork.0);
    pending.extend_entities(display_batch(
        pool,
        stream,
        artwork,
        step.partial_tick,
        &scratch.displays,
    ));
    pool.scratch = scratch;
}

/// Whether any part of the spawner block at `position` lies in the view; fails open without one.
fn block_in_view(view: Option<ActorCullView>, position: [i32; 3]) -> bool {
    let [x, y, z] = position.map(|axis| axis as f32);
    render_model::actor_bounds_are_visible(
        [x + 0.5, y, z + 0.5],
        1.0,
        assets::SkinGeometryBounds {
            center: [0.0, 0.5, 0.0],
            half_extents: [0.5; 3],
        },
        view,
    )
}

/// Returns the centre of the spawner block at `position`.
fn display_center(position: [i32; 3]) -> Vec3 {
    Vec3::from_array(position.map(|value| value as f32)) + Vec3::splat(0.5)
}

/// Converts and places display poses, then resolves their lighting and controller layers.
fn display_batch(
    pool: &mut Pool,
    stream: &chunk_pipeline::WorldStream,
    artwork: &ActorArtworkPages,
    partial_tick: f32,
    displays: &[Display],
) -> ActorPresentationBatch {
    let mut batch = ActorPresentationBatch {
        submissions: Vec::new(),
        skin_layers: Vec::new(),
        artwork: Default::default(),
    };
    pool.poses.begin_frame();
    let mut sampled = pool.actors.frame(partial_tick);
    for &(runtime, position, spin) in displays {
        let (Some(actor), Some(rig)) = (pool.actors.actor(runtime), pool.actors.rig(runtime))
        else {
            continue;
        };
        let rig = sampled.sample_rig_scale(rig);
        let Some(mut presentation) = entity_rig_presentation_cached(
            &rig,
            actor,
            artwork,
            partial_tick,
            Some(&mut pool.poses),
        ) else {
            continue;
        };
        let dimensions = presentation
            .submission
            .culling_bounds
            .half_extents
            .map(|axis| axis * 2.0);
        let dimensions = std::array::from_fn(|axis| {
            dimensions[axis] * rig.scale * actor.render_scale() * rig.axis_scale[axis].abs()
        });
        let Some(placement) = spawner_actor_transform(position, dimensions, spin, partial_tick)
        else {
            continue;
        };
        let body = &mut presentation.submission;
        let [x, y, z] = body.world_from_actor;
        let normal = Mat4::from_cols_array(&[
            x[0], y[0], z[0], 0.0, x[1], y[1], z[1], 0.0, x[2], y[2], z[2], 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
        body.world_from_actor = matrix_rows(placement * normal);
        if let Some(location) = presentation.artwork {
            batch.artwork.insert(body.input.identity, location);
        }
        batch.submissions.push(presentation.submission);
    }
    for body in &mut batch.submissions {
        let feet = body.world_from_actor.map(|row| row[3]);
        let position = pool
            .actors
            .actor(body.input.identity.runtime_id)
            .map_or(feet, |actor| actor.brightness_sample_position(feet));
        if let Some((block, sky)) = stream.solved_light_at(position) {
            body.light = render_model::pack_actor_light(block, sky);
        }
    }
    pool.layers.begin_frame();
    apply_render_layers_cached(
        &mut batch,
        |runtime| sampled.layers(runtime),
        artwork,
        &mut pool.layers,
    );
    batch
}

#[cfg(test)]
mod tests;
