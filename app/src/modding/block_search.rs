//! Answers mods' `terrain.find-blocks` searches from loaded terrain, with bounded scan work
//! per frame shared by every mod.
use mod_host::{BlockSearch, FoundBlocks, TerrainPosition, TerrainReadError};

/// Incremental scan steps per frame, shared by every mod's searches; new searches go first.
const SCAN_UPDATES_PER_FRAME: usize = 2;

/// One distinct identifier set and range, shared by every mod searching for it.
struct Scan {
    identifiers: Vec<String>,
    range: f32,
    ids: Vec<u32>,
    scan: world::LoadedBlockScan,
    /// The latest matches, nearest first, at most `MAX_FOUND_BLOCKS`.
    found: Vec<[i32; 3]>,
    /// The frame of the latest update; none before the first.
    updated: Option<u64>,
    requested: bool,
}

/// The world a scan belongs to: session, dimension, block-name revision, asset identity and
/// runtime-ID mode. Any change restarts every scan.
type Scope = (u64, i32, u64, usize, assets::NetworkIdMode);

/// Scans of loaded terrain behind every mod's block searches, in one world.
#[derive(Default)]
pub(super) struct BlockSearches {
    scope: Option<Scope>,
    scans: Vec<Scan>,
    frame: u64,
}

impl BlockSearches {
    /// Answers every terrain-authorized mod's searches, then advances the stalest scans.
    pub(super) fn refresh(
        &mut self,
        hosts: &mut [&mut mod_host::ModHost],
        stream: &chunk_pipeline::WorldStream,
        observation: &mod_host::WorldObservation,
    ) {
        let authority = stream.authority();
        let scope = (
            observation.session,
            observation.dimension,
            authority.block_name_revision(),
            std::sync::Arc::as_ptr(stream.runtime_assets()) as usize,
            stream.network_id_mode(),
        );
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.scans.clear();
        }
        self.frame += 1;
        for scan in &mut self.scans {
            scan.requested = false;
        }
        let (x, y, z) = observation.position;
        let mut source = Source {
            searches: self,
            authority,
            feet: [x, y, z],
            session: observation.session,
            dimension: observation.dimension,
            updates: 0,
        };
        mod_host::refresh_block_searches(hosts, &mut source);
        let remaining = SCAN_UPDATES_PER_FRAME.saturating_sub(source.updates);
        self.scans.retain(|scan| scan.requested);
        let mut stalest: Vec<_> = (0..self.scans.len())
            .filter(|index| self.scans[*index].updated != Some(self.frame))
            .collect();
        stalest.sort_by_key(|index| self.scans[*index].updated);
        for index in stalest.into_iter().take(remaining) {
            self.update(index, authority.terrain(), [x, y, z], observation.dimension);
        }
    }

    /// Advances one scan by its bounded per-call work around the feet.
    fn update(&mut self, index: usize, store: &world::ChunkStore, feet: [f32; 3], dimension: i32) {
        let scan = &mut self.scans[index];
        scan.found.clear();
        scan.found.extend_from_slice(scan.scan.update(
            store,
            dimension,
            feet,
            scan.range,
            &scan.ids,
            mod_api::MAX_FOUND_BLOCKS,
        ));
        scan.updated = Some(self.frame);
    }
}

/// This frame's world, as one round of block searches sees it.
struct Source<'a> {
    searches: &'a mut BlockSearches,
    authority: &'a client_world::WorldAuthority,
    feet: [f32; 3],
    session: u64,
    dimension: i32,
    updates: usize,
}

impl mod_host::BlockSearchSource for Source<'_> {
    /// Shares one scan per identifier set and range; a new scan takes its first step at once.
    fn find(&mut self, search: &BlockSearch) -> Result<FoundBlocks, TerrainReadError> {
        let searches = &mut *self.searches;
        let index =
            match searches.scans.iter().position(|scan| {
                scan.identifiers == search.identifiers && scan.range == search.range
            }) {
                Some(index) => index,
                None => {
                    let mut ids: Vec<u32> = search
                        .identifiers
                        .iter()
                        .flat_map(|name| self.authority.block_ids_for_name(name).iter().copied())
                        .collect();
                    ids.sort_unstable();
                    ids.dedup();
                    searches.scans.push(Scan {
                        identifiers: search.identifiers.clone(),
                        range: search.range,
                        ids,
                        scan: Default::default(),
                        found: Vec::new(),
                        updated: None,
                        requested: false,
                    });
                    searches.scans.len() - 1
                }
            };
        searches.scans[index].requested = true;
        if searches.scans[index].updated.is_none() && self.updates < SCAN_UPDATES_PER_FRAME {
            self.updates += 1;
            searches.update(index, self.authority.terrain(), self.feet, self.dimension);
        }
        Ok(FoundBlocks {
            session: self.session,
            dimension: self.dimension,
            positions: searches.scans[index]
                .found
                .iter()
                .take(search.limit as usize)
                .map(|&[x, y, z]| TerrainPosition { x, y, z })
                .collect(),
        })
    }
}
