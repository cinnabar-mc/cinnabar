//! Read-only native BDS companion. No requests are sent without an actively read subscription.
//!
//! The pack is world-local. A missing/disabled Script API remains recoverably unavailable. No shared
//! installation resources, cheats, experiments or script permissions are changed here.
use std::{
    fs,
    io::{self, Read},
    path::Path,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use mod_api::local_terrain::{
    Error, MAX_CELLS, MAX_COORDINATE, MAX_EXTENT, MAX_METADATA_BYTES, MAX_NAME_BYTES, MAX_QUERIES,
    MAX_STATE_BYTES, Query, Region, Source, UNKNOWN, VERSION,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{lock, process::Process, write_private};

const PACK_ID: &str = "8929357d-ccf6-4774-9688-5c1468842d80";
const MODULE_ID: &str = "53055615-99a2-47e6-9f13-a618be261b90";
const PACK_VERSION: [u32; 3] = [1, 0, 0];
const PREFIX: &str = "CINNABAR_TERRAIN ";
const EVENT: &str = "cinnabar_terrain:sync";
const PART_BYTES: usize = 2048;
pub(super) const MAX_FRAME_BYTES: usize = PART_BYTES * 4;
const MAX_RESPONSE_BYTES: usize = MAX_METADATA_BYTES * 6 + MAX_CELLS * 6 + 4096;
const TTL: Duration = Duration::from_secs(2);
const RESPONSE_TTL: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(250);
static SESSION: AtomicU64 = AtomicU64::new(1);

pub(super) fn is_frame(line: &str) -> bool {
    line.contains(PREFIX)
}

/// Installs only the companion's private directory and merges validated world pack references.
pub(super) fn install(world: &Path) -> io::Result<Arc<Bridge>> {
    let bridge = Bridge::new()?;
    let references = world.join("world_behavior_packs.json");
    let existing = match read_bounded(&references) {
        Ok(bytes) if bytes.len() <= MAX_METADATA_BYTES => serde_json::from_slice(&bytes)?,
        Ok(_) => {
            return Err(io::Error::other(
                "world behavior pack list exceeds companion budget",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => json!([]),
        Err(error) => return Err(error),
    };
    let merged = merge_packs(existing)?;
    let pack = world.join("behavior_packs/cinnabar_local_terrain");
    if pack.exists() {
        let manifest: Value = serde_json::from_slice(&read_bounded(&pack.join("manifest.json"))?)?;
        if manifest["header"]["uuid"] != PACK_ID {
            return Err(io::Error::other(
                "companion directory belongs to another pack",
            ));
        }
    }
    super::create_private_dir(&pack.join("scripts"))?;
    let manifest = json!({
        "format_version": 2,
        "header": {"name": "Cinnabar local terrain", "description": "Bounded local terrain observations",
            "uuid": PACK_ID, "version": PACK_VERSION, "min_engine_version": [1, 26, 0]},
        "modules": [{"type": "script", "language": "javascript", "uuid": MODULE_ID,
            "version": PACK_VERSION, "entry": "scripts/main.js"}],
        "dependencies": [{"module_name": "@minecraft/server", "version": "2.6.0"}]
    });
    write_private(
        &pack.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    let config = json!({"version": VERSION, "session": bridge.session.to_string(), "nonce": bridge.nonce,
        "maxExtent": MAX_EXTENT, "maxQueries": MAX_QUERIES, "maxCells": MAX_CELLS,
        "maxMetadataBytes": MAX_METADATA_BYTES, "unknown": UNKNOWN, "prefix": PREFIX,
        "event": EVENT, "partBytes": PART_BYTES, "maxResponseBytes": MAX_RESPONSE_BYTES,
        "ttlTicks": TTL.as_secs() * mod_api::BEDROCK_TICKS_PER_SECOND as u64, "cellsPerTick": 512,
        "maxNameBytes": MAX_NAME_BYTES, "maxStateBytes": MAX_STATE_BYTES, "maxCoordinate": MAX_COORDINATE,
        "retryTicks": mod_api::BEDROCK_TICKS_PER_SECOND, "dimensions": mod_api::local_terrain::DIMENSIONS});
    write_private(
        &pack.join("scripts/config.js"),
        format!("export default {config};\n").as_bytes(),
    )?;
    write_private(
        &pack.join("scripts/main.js"),
        include_bytes!("local_terrain/main.js"),
    )?;
    // Rename keeps malformed/interrupted writes from replacing a valid reference list.
    let temporary = world.join(".cinnabar-terrain-packs.tmp");
    write_private(&temporary, &serde_json::to_vec_pretty(&merged)?)?;
    fs::rename(temporary, references)?;
    Ok(bridge)
}

fn merge_packs(mut value: Value) -> io::Result<Value> {
    let entries = value
        .as_array_mut()
        .ok_or_else(|| io::Error::other("invalid world behavior pack list"))?;
    for entry in entries.iter() {
        let valid = entry.as_object().is_some()
            && entry["pack_id"].as_str().is_some_and(valid_uuid)
            && entry["version"].as_array().is_some_and(|version| {
                version.len() == 3 && version.iter().all(|n| n.as_u64().is_some())
            });
        if !valid {
            return Err(io::Error::other("invalid world behavior pack reference"));
        }
    }
    entries.retain(|entry| entry["pack_id"] != PACK_ID);
    entries.push(json!({"pack_id": PACK_ID, "version": PACK_VERSION}));
    Ok(value)
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_METADATA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(io::Error::other("pack metadata exceeds companion budget"));
    }
    Ok(bytes)
}

fn valid_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

struct Subscription {
    id: u32,
    query: Query,
    touched: Instant,
    received: Option<Instant>,
    region: Option<Arc<Region>>,
    failed: bool,
    sequence: u64,
    assembly: Option<Assembly>,
}

struct Assembly {
    sequence: u64,
    total: usize,
    parts: Vec<String>,
    bytes: usize,
}

#[derive(Default)]
struct State {
    subscriptions: Vec<Subscription>,
    next_id: u32,
    next_revision: u64,
}

/// Process-owned transport state. The process and worker never retain the granted reader.
pub(super) struct Bridge {
    session: u64,
    nonce: String,
    state: Mutex<State>,
    reader: Mutex<Weak<Reader>>,
    stopped: AtomicBool,
    cleared: (
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
    ),
}

impl Bridge {
    fn new() -> io::Result<Arc<Self>> {
        let mut nonce = [0_u8; 24];
        getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Arc::new(Self {
            session: SESSION.fetch_add(1, Ordering::Relaxed),
            nonce: nonce.iter().map(|byte| format!("{byte:02x}")).collect(),
            state: Mutex::new(State::default()),
            reader: Mutex::new(Weak::new()),
            stopped: AtomicBool::new(false),
            cleared: crossbeam_channel::bounded(1),
        }))
    }

    pub(super) fn source(self: &Arc<Self>) -> Option<Arc<dyn Source>> {
        if self.stopped.load(Ordering::Acquire) {
            return None;
        }
        let mut weak = self.reader.try_lock().ok()?;
        if let Some(reader) = weak.upgrade() {
            return Some(reader);
        }
        let mut state = self.state.try_lock().ok()?;
        if self.stopped.load(Ordering::Acquire) {
            return None;
        }
        state.subscriptions.clear();
        let reader = Arc::new(Reader(Arc::clone(self)));
        *weak = Arc::downgrade(&reader);
        Some(reader)
    }

    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let mut state = lock(&self.state);
        state.subscriptions.clear();
    }

    pub(super) fn active(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
    }

    /// Drains a bounded subscription snapshot off the frame thread, with no queued console work.
    pub(super) fn start(self: &Arc<Self>, process: &Arc<Process>) {
        let bridge = Arc::clone(self);
        let process = Arc::downgrade(process);
        let spawned = thread::Builder::new()
            .name("local-terrain".into())
            .spawn(move || {
                let ticker = crossbeam_channel::tick(POLL);
                let mut had_queries = false;
                while ticker.recv().is_ok() {
                    let Some(process) = process.upgrade() else {
                        break;
                    };
                    if process.terrain_stopped() {
                        bridge.stop();
                        let body = json!({"version": VERSION, "session": bridge.session.to_string(),
                        "nonce": bridge.nonce, "queries": [], "stop": true});
                        while bridge.cleared.1.try_recv().is_ok() {}
                        let _ = process.send(&format!("scriptevent {EVENT} {body}"));
                        // Graceful shutdown gives the script a chance to remove even pending areas.
                        let _ = bridge.cleared.1.recv_timeout(TTL);
                        let _ = process.send("stop");
                        break;
                    }
                    let queries = bridge.snapshot(Instant::now());
                    if queries.is_empty() && !had_queries {
                        continue;
                    }
                    had_queries = !queries.is_empty();
                    let body = json!({"version": VERSION, "session": bridge.session.to_string(),
                    "nonce": bridge.nonce, "queries": queries});
                    if process
                        .send(&format!("scriptevent {EVENT} {body}"))
                        .is_err()
                    {
                        bridge.stop();
                        break;
                    }
                }
            });
        if spawned.is_err() {
            self.stop();
        }
    }

    fn snapshot(&self, now: Instant) -> Vec<Value> {
        let has_reader = lock(&self.reader).strong_count() != 0;
        let mut state = lock(&self.state);
        if self.stopped.load(Ordering::Acquire) || !has_reader {
            state.subscriptions.clear();
        }
        state
            .subscriptions
            .retain(|sub| now.duration_since(sub.touched) < TTL);
        state
            .subscriptions
            .iter()
            .map(|sub| json!({"id": sub.id, "query": sub.query}))
            .collect()
    }

    /// Accepts only bounded, ordered fragments from this launch and an active request id.
    pub(super) fn observe(&self, line: &str) {
        let Some((_, body)) = line.split_once(PREFIX) else {
            return;
        };
        if body.len() > MAX_FRAME_BYTES {
            return;
        }
        let Ok(frame) = serde_json::from_str::<Frame>(body.trim()) else {
            return;
        };
        if frame.version != VERSION
            || frame.session != self.session.to_string()
            || frame.nonce != self.nonce
        {
            return;
        }
        if frame.id == 0
            && frame.sequence == 0
            && frame.part == 0
            && frame.total == 1
            && frame.data == "stop"
        {
            let _ = self.cleared.0.try_send(());
            return;
        }
        let mut state = lock(&self.state);
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let State {
            subscriptions,
            next_revision,
            ..
        } = &mut *state;
        let Some(sub) = subscriptions.iter_mut().find(|sub| sub.id == frame.id) else {
            return;
        };
        if frame.sequence <= sub.sequence
            || frame.total == 0
            || frame.total > MAX_RESPONSE_BYTES.div_ceil(PART_BYTES)
            || frame.part >= frame.total
            || frame.data.len() > PART_BYTES
        {
            return;
        }
        if frame.part == 0 {
            if sub
                .assembly
                .as_ref()
                .is_some_and(|a| a.sequence >= frame.sequence)
            {
                return;
            }
            sub.assembly = Some(Assembly {
                sequence: frame.sequence,
                total: frame.total,
                parts: Vec::new(),
                bytes: 0,
            });
        }
        let Some(assembly) = sub.assembly.as_mut() else {
            return;
        };
        if assembly.sequence != frame.sequence
            || assembly.total != frame.total
            || assembly.parts.len() != frame.part
        {
            return;
        }
        assembly.bytes += frame.data.len();
        if assembly.bytes > MAX_RESPONSE_BYTES {
            sub.assembly = None;
            return;
        }
        assembly.parts.push(frame.data);
        if assembly.parts.len() != assembly.total {
            return;
        }
        let assembly = sub.assembly.take().expect("complete assembly");
        let Ok(reply) = serde_json::from_str::<Reply>(&assembly.parts.concat()) else {
            return;
        };
        sub.sequence = assembly.sequence;
        if reply.error {
            sub.failed = true;
            return;
        }
        let Some(mut region) = reply.region else {
            return;
        };
        if region.session != self.session
            || !region.valid(&sub.query)
            || !canonical_palette(&region)
        {
            return;
        }
        for block in &mut region.palette {
            let raw: serde_json::Map<String, Value> =
                serde_json::from_str(&block.states).expect("validated states");
            let canonical: serde_json::Map<String, Value> = raw
                .into_iter()
                .map(|(key, value)| {
                    let typed = match value {
                        Value::Bool(value) => json!({"type": "byte", "value": u8::from(value)}),
                        Value::String(value) => json!({"type": "string", "value": value}),
                        value => json!({"type": "int", "value": value}),
                    };
                    (key, typed)
                })
                .collect();
            block.states = serde_json::to_string(&canonical).expect("JSON states");
        }
        if !region.valid(&sub.query) {
            return;
        }
        region.revision = if let Some(old) = &sub.region
            && old.palette == region.palette
            && old.cells == region.cells
        {
            old.revision
        } else {
            let Some(revision) = next_revision.checked_add(1) else {
                return;
            };
            *next_revision = revision;
            revision
        };
        if sub
            .region
            .as_ref()
            .is_none_or(|old| old.revision != region.revision)
        {
            sub.region = Some(Arc::new(region));
        }
        sub.failed = false;
        sub.received = Some(Instant::now());
    }
}

fn canonical_palette(region: &Region) -> bool {
    region.palette.iter().all(|block| {
        serde_json::from_str::<Value>(&block.states)
            .ok()
            .is_some_and(|value| {
                value.as_object().is_some_and(|object| {
                    object.values().all(|v| {
                        v.is_boolean()
                            || v.is_string()
                            || v.as_i64().is_some_and(|value| i32::try_from(value).is_ok())
                    })
                }) && serde_json::to_string(&value).ok().as_deref() == Some(block.states.as_str())
            })
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u32,
    session: String,
    nonce: String,
    id: u32,
    sequence: u64,
    part: usize,
    total: usize,
    data: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    error: bool,
    region: Option<Region>,
}

struct Reader(Arc<Bridge>);
impl Source for Reader {
    fn read(&self, query: &Query) -> Result<Option<Arc<Region>>, Error> {
        if !query.valid() {
            return Err(Error::InvalidQuery);
        }
        let now = Instant::now();
        let mut state = self
            .0
            .state
            .try_lock()
            .map_err(|_| Error::BudgetExhausted)?;
        if self.0.stopped.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        state
            .subscriptions
            .retain(|sub| now.duration_since(sub.touched) < TTL);
        if let Some(sub) = state
            .subscriptions
            .iter_mut()
            .find(|sub| sub.query == *query)
        {
            sub.touched = now;
            if sub.failed
                || sub
                    .received
                    .is_none_or(|at| now.duration_since(at) > RESPONSE_TTL)
            {
                return Err(Error::Unavailable);
            }
            return Ok(sub.region.clone());
        }
        if state.subscriptions.len() >= MAX_QUERIES {
            return Err(Error::BudgetExhausted);
        }
        state.next_id = state.next_id.checked_add(1).ok_or(Error::BudgetExhausted)?;
        let id = state.next_id;
        state.subscriptions.push(Subscription {
            id,
            query: query.clone(),
            touched: now,
            received: None,
            region: None,
            failed: false,
            sequence: 0,
            assembly: None,
        });
        Ok(None)
    }
}

#[cfg(test)]
#[path = "local_terrain/tests.rs"]
mod tests;
