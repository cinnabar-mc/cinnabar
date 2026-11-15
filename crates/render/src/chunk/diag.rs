//! Opt-in opaque terrain diagnostics for phone GPU investigations. Each switch is read about
//! once a second from the `debug.cinnabar.diag_*` Android system property (`adb shell setprop`)
//! or the matching `CINNABAR_DIAG_*` environment variable, so it can be flipped while the client
//! runs. Every switch is off by default; its state is logged whenever it changes.
//! Logs go to stdout, which the Android build forwards to logcat.
#![allow(clippy::print_stdout)]

use std::sync::{
    RwLock,
    atomic::{AtomicU32, Ordering},
};

use bevy::platform::collections::HashMap;
use bevy::render::render_resource::{
    CachedRenderPipelineId, PipelineCache, RenderPipelineDescriptor,
};

/// The current switch values; all zero (off) by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct TerrainDiag {
    /// (a) `diag_solid_trivial=1`: solid terrain uses a constant-colour fragment shader.
    pub(in crate::chunk) solid_trivial: bool,
    /// (b) `diag_cutout_as_solid=1`: cutout terrain uses the solid fragment shader (no discard).
    pub(in crate::chunk) cutout_as_solid: bool,
    /// (c) `diag_solid_first=1`: the solid stream is queued before every other opaque stream.
    pub(in crate::chunk) solid_first: bool,
    /// (c') `diag_front_to_back=1`: solid and cutout sections are submitted nearest first.
    pub(in crate::chunk) front_to_back: bool,
    /// (d) `diag_skip_streams=MASK`: skips opaque streams; 1 solid, 2 cutout, 4 model, 8 liquid.
    pub(in crate::chunk) skip_streams: u8,
    /// (e) `diag_no_split=1`: never opens the separate direct-occlusion terrain pass.
    pub(in crate::chunk) no_split: bool,
    /// (f) `diag_vertex=1`: minimal terrain vertex shader (position only, constant varyings);
    /// `diag_vertex=2`: every terrain vertex is rejected before any fetch (nothing rasterises);
    /// `diag_vertex=3`: the full vertex shader without the per-vertex mixed biome tint blend
    /// (its faces then blend per fragment instead); `diag_vertex=4`: no mixed biome blend in
    /// either stage, so one-block tinted faces in mixed records render untinted;
    /// `diag_vertex=5`: every per-quad record lookup (positional variation, material,
    /// animation frames, masks, biome tint) reads one fixed address instead, keeping the
    /// per-vertex light, AO, UV and position work: the most a pre-baked per-quad record saves
    /// (terrain shows the missing-texture checker); `diag_vertex=6`: the quad's own material
    /// record (real textures and flags) without the positional-variation loop or biome tint.
    pub(in crate::chunk) vertex: u8,
    /// (g) `diag_max_chunks=N`: draws only sections within N chunks (horizontal) of the camera.
    pub(in crate::chunk) max_chunks: u32,
    /// (g') `diag_keep_stride=K`: draws only about one section in K.
    pub(in crate::chunk) keep_stride: u32,
    /// (i)-(iii) `diag_sample=1`: terrain textures use `textureSampleLevel` with a level computed
    /// from the same gradients; `2`: implicit-derivative `textureSample`; `3`: level 0; `4`:
    /// the lowest mip. Pixels may differ; these separate sample-instruction from cache cost.
    pub(in crate::chunk) sample: u8,
    /// (iv) `diag_page0=1`: terrain textures sample texture page 0 only (wrong pixels).
    pub(in crate::chunk) page0: bool,
    /// `diag_shade=N` drops one part of non-Enhanced cube shading, keeping the rest:
    /// 1 lightmap (constant light), 2 fog, 3 gamma-to-linear conversion, 4 per-fragment tint and
    /// glow/emissive masks, 5 lightmap as one nearest table read instead of four, 6 dynamic
    /// point-light loop only, 7 everything (reads every input, no texture or table), 8 1-4 at once,
    /// 9 only the per-fragment biome blend fallback (flat face tints stay), 10 only the
    /// glow/emissive mask samples.
    pub(in crate::chunk) shade: u8,
    /// `diag_vary=N` splits vertex work from the varying interface on the unit vertex path
    /// (non-Enhanced pipelines only): 1 full vertex work folded into one flat word, the fragment
    /// shading constants; 2 only the position chain, every varying written with cheap unfoldable
    /// values; 3 only the position chain and one flat word; 4 packed varyings (no
    /// `local_position`, tint in one word).
    pub(in crate::chunk) vary: u8,
    /// `diag_flat_units=1`: unit pages hold each pass's units contiguously (the non-indexed
    /// layout) and the vertex shader reads descriptors directly, two dependent storage-load
    /// levels instead of four.
    pub(in crate::chunk) flat_units: bool,
    /// `diag_frame=MASK` removes non-terrain work around the opaque and hand passes; see
    /// [`FrameSkip`].
    pub(in crate::chunk) frame: u32,
    /// `diag_sky=MASK`: 1 draws the sky after opaque and cutout geometry, so only uncovered
    /// pixels shade it (not while a pack skybox is active); 2 uses the sky shader that samples
    /// the sun and moon only inside their sprites and reads the fan rim without a dynamically
    /// indexed array. Both keep the image identical.
    pub(in crate::chunk) sky: u8,
    /// `diag_ui_categories=1`: times UI raster, model, composite, invert and backdrop passes
    /// separately instead of as one `ui` stage.
    pub(in crate::chunk) ui_categories: bool,
    /// `diag_units=0`: disables batched unit draws, falling back to per-section direct draws.
    pub(in crate::chunk) units_off: bool,
    /// (h) `diag_counts=1`: logs terrain draw, instance and vertex counts every 120 frames.
    pub(in crate::chunk) counts: bool,
}

static STATE: RwLock<TerrainDiag> = RwLock::new(TerrainDiag {
    solid_trivial: false,
    cutout_as_solid: false,
    solid_first: false,
    front_to_back: false,
    skip_streams: 0,
    no_split: false,
    vertex: 0,
    max_chunks: 0,
    keep_stride: 0,
    sample: 0,
    page0: false,
    shade: 0,
    vary: 0,
    flat_units: false,
    frame: 0,
    sky: 0,
    ui_categories: false,
    units_off: false,
    counts: false,
});
static FRAME: AtomicU32 = AtomicU32::new(0);

/// The switch values as of the last refresh.
pub(in crate::chunk) fn current() -> TerrainDiag {
    // Render apps without terrain views never call `refresh`; read the switches once anyway.
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(|| *STATE.write().unwrap_or_else(|poison| poison.into_inner()) = read());
    *STATE.read().unwrap_or_else(|poison| poison.into_inner())
}

/// Re-reads the switches every 60 calls (and on the first), logging any change. While
/// `diag_ab` is set, also selects this frame's A or B state.
pub(in crate::chunk) fn refresh() {
    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
    if frame.is_multiple_of(60) {
        ab::reload();
    }
    if let Some(state) = ab::state_for(frame) {
        *STATE.write().unwrap_or_else(|poison| poison.into_inner()) = state;
        return;
    }
    if !frame.is_multiple_of(60) {
        return;
    }
    let next = read();
    let mut state = STATE.write().unwrap_or_else(|poison| poison.into_inner());
    if frame == 0 || *state != next {
        println!("CINNABAR_TERRAIN_DIAG {next:?}");
        *state = next;
    }
}

/// In-process A/B timing: `diag_ab=SPEC` alternates between the ordinary switches (A) and the
/// same switches with SPEC applied (B), for example `vertex=5,vary=4`, `vertex=5+vary=4` or
/// `sky:2`, every `diag_ab_frames` frames (default 60), ignoring the first `diag_ab_skip`
/// frames (default 10) of each half and the first two A/B cycles after the spec changes, while
/// B's pipeline variants compile. Per-pass GPU times are averaged per state and logged every 5 s as
/// `CINNABAR_AB`; `CINNABAR_PASS_TIMES` logs the averages of all frames, A/B or not.
pub(crate) mod ab {
    use super::{FRAME, TerrainDiag, read, read_with, value};
    use render_scene::{GpuFrameTimes, RuntimeStage};
    use std::{
        collections::VecDeque,
        sync::{Mutex, RwLock, atomic::Ordering},
        time::{Duration, Instant},
    };

    const REPORT_EVERY: Duration = Duration::from_secs(5);

    /// The parsed A/B request and both states it alternates between.
    struct Plan {
        spec: String,
        base: TerrainDiag,
        b: TerrainDiag,
        period: u32,
        skip: u32,
        /// The refresh count when this spec started; earlier cycles cover first compiles.
        start: u32,
    }

    static PLAN: RwLock<Option<Plan>> = RwLock::new(None);

    /// Which state a frame ran in.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Tag {
        A,
        B,
        Skip,
    }

    /// Sums of per-pass milliseconds over the frames of one state.
    struct Sums {
        frames: u32,
        ms: [f64; RuntimeStage::GPU.len()],
    }

    impl Sums {
        const fn new() -> Self {
            Self {
                frames: 0,
                ms: [0.0; RuntimeStage::GPU.len()],
            }
        }

        fn add(&mut self, frame: &GpuFrameTimes) {
            self.frames += 1;
            for (index, stage) in RuntimeStage::GPU.iter().enumerate() {
                if let Some(duration) = frame.get(*stage) {
                    self.ms[index] += duration.as_secs_f64() * 1000.0;
                }
            }
        }

        /// Average milliseconds per frame of stage `index`, over every frame of this state.
        fn average(&self, index: usize) -> f64 {
            self.ms[index] / f64::from(self.frames.max(1))
        }
    }

    struct Accumulator {
        /// Frames whose GPU work is still being timed, with the state they ran in.
        pending: VecDeque<(u64, Tag)>,
        all: Sums,
        a: Sums,
        b: Sums,
        since: Option<Instant>,
    }

    static ACCUMULATOR: Mutex<Accumulator> = Mutex::new(Accumulator {
        pending: VecDeque::new(),
        all: Sums::new(),
        a: Sums::new(),
        b: Sums::new(),
        since: None,
    });

    /// Re-reads `diag_ab` and the switches it alternates; logs when the request changes.
    pub(super) fn reload() {
        let spec = value("AB").map(|spec| spec.trim().to_owned());
        let mut plan = PLAN.write().unwrap_or_else(|poison| poison.into_inner());
        let Some(spec) = spec.filter(|spec| !spec.is_empty()) else {
            if plan.take().is_some() {
                println!("CINNABAR_AB stopped");
            }
            return;
        };
        let overrides = spec
            .split([',', '+', ';', ' '])
            .filter_map(|pair| {
                let (name, value) = pair.split_once(['=', ':'])?;
                Some((name.trim().to_ascii_uppercase(), value.trim().to_owned()))
            })
            .collect::<Vec<_>>();
        let number = |name: &str, default: u32| {
            value(name)
                .and_then(|value| value.trim().parse::<u32>().ok())
                .unwrap_or(default)
        };
        let period = number("AB_FRAMES", 60).max(2);
        let skip = number("AB_SKIP", 10).min(period - 1);
        let base = read();
        let b = read_with(&|name| {
            overrides
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .or_else(|| value(name))
        });
        let start = match plan.as_ref() {
            Some(old) if old.spec == spec && old.period == period && old.skip == skip => old.start,
            _ => {
                println!("CINNABAR_AB start spec={spec} frames={period} skip={skip} b={b:?}");
                FRAME.load(Ordering::Relaxed)
            }
        };
        *plan = Some(Plan {
            spec,
            base,
            b,
            period,
            skip,
            start,
        });
    }

    /// The A or B state of `frame` while an A/B request is active.
    pub(super) fn state_for(frame: u32) -> Option<TerrainDiag> {
        let plan = PLAN.read().unwrap_or_else(|poison| poison.into_inner());
        let plan = plan.as_ref()?;
        Some(if (frame / plan.period) % 2 == 1 {
            plan.b
        } else {
            plan.base
        })
    }

    /// Records which state the frame `frame_id` (the GPU timing frame id) is drawn in.
    pub(crate) fn note_frame(frame_id: u64) {
        let tag = {
            let plan = PLAN.read().unwrap_or_else(|poison| poison.into_inner());
            let Some(plan) = plan.as_ref() else {
                return;
            };
            let frame = FRAME.load(Ordering::Relaxed).wrapping_sub(1);
            if frame.wrapping_sub(plan.start) < 4 * plan.period || frame % plan.period < plan.skip {
                Tag::Skip
            } else if (frame / plan.period) % 2 == 1 {
                Tag::B
            } else {
                Tag::A
            }
        };
        let mut accumulator = ACCUMULATOR
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        accumulator.pending.push_back((frame_id, tag));
        while accumulator.pending.len() > 64 {
            accumulator.pending.pop_front();
        }
    }

    /// Adds a completed frame's GPU times and logs the averages every five seconds.
    pub(crate) fn record(frame_id: u64, frame: &GpuFrameTimes) {
        if frame.iter().next().is_none() {
            return;
        }
        let mut accumulator = ACCUMULATOR
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let accumulator = &mut *accumulator;
        let mut tag = None;
        while let Some(&(id, frame_tag)) = accumulator.pending.front() {
            if id > frame_id {
                break;
            }
            accumulator.pending.pop_front();
            if id == frame_id {
                tag = Some(frame_tag);
            }
        }
        accumulator.all.add(frame);
        match tag {
            Some(Tag::A) => accumulator.a.add(frame),
            Some(Tag::B) => accumulator.b.add(frame),
            _ => {}
        }
        let now = Instant::now();
        let since = *accumulator.since.get_or_insert(now);
        if now.duration_since(since) < REPORT_EVERY {
            return;
        }
        let name = |stage: RuntimeStage| match stage {
            RuntimeStage::GpuFrame => "total",
            stage => stage.name().strip_prefix("gpu_").unwrap_or(stage.name()),
        };
        let mut line = format!("CINNABAR_PASS_TIMES frames={}", accumulator.all.frames);
        for (index, stage) in RuntimeStage::GPU.iter().enumerate() {
            if accumulator.all.ms[index] > 0.0 {
                line += &format!(" {}={:.2}", name(*stage), accumulator.all.average(index));
            }
        }
        println!("{line}");
        let spec = PLAN
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .map(|plan| plan.spec.clone());
        if let Some(spec) = spec {
            let (a, b) = (&accumulator.a, &accumulator.b);
            let mut line = format!(
                "CINNABAR_AB spec={spec} a_frames={} b_frames={}",
                a.frames, b.frames
            );
            for (index, stage) in RuntimeStage::GPU.iter().enumerate() {
                if a.ms[index] > 0.0 || b.ms[index] > 0.0 {
                    let stage = name(*stage);
                    line += &format!(
                        " a_{stage}={:.2} b_{stage}={:.2}",
                        a.average(index),
                        b.average(index)
                    );
                }
            }
            println!("{line}");
        }
        accumulator.all = Sums::new();
        accumulator.a = Sums::new();
        accumulator.b = Sums::new();
        accumulator.since = Some(now);
    }
}

/// Bits of `diag_frame`, each removing one piece of non-terrain world work for phone timing.
/// Pixels are wrong while any is set.
pub(crate) struct FrameSkip;

impl FrameSkip {
    /// The sky: stock sky triangle, stars and pack skybox.
    pub(crate) const SKY: u32 = 1;
    /// Clouds.
    pub(crate) const CLOUDS: u32 = 2;
    /// Entities (actors) in the opaque phase.
    pub(crate) const ENTITIES: u32 = 4;
    /// Block entities and dropped items in the opaque phase.
    pub(crate) const BLOCK_ENTITIES: u32 = 8;
    /// Both first-person hand passes.
    pub(crate) const HANDS: u32 = 16;
    /// The opaque pass discards depth instead of storing it; later depth readers see garbage.
    pub(crate) const OPAQUE_DEPTH_DISCARD: u32 = 32;
    /// The hand passes clear colour instead of loading the scene (the scene is lost).
    pub(crate) const HAND_CLEAR: u32 = 64;
    /// Only the pack skybox (frag_depth writer), keeping the stock sky and stars.
    pub(crate) const PACK_SKYBOX: u32 = 128;
    /// Only the star geometry, which draws by day too (its fragments then discard).
    pub(crate) const STARS: u32 = 256;
    /// The hand passes open, clear and store as usual but issue no draw.
    pub(crate) const HAND_NO_DRAW: u32 = 512;
}

/// Prints one diagnostic line for logcat.
pub(crate) fn log(line: std::fmt::Arguments<'_>) {
    println!("{line}");
}

/// `diag_sky` bit 1: the sky draws after opaque geometry.
pub(crate) fn sky_late() -> bool {
    current().sky & 1 != 0
}

/// `diag_sky` bit 2: the sky shader's fast variant.
pub(crate) fn sky_fast() -> bool {
    current().sky & 2 != 0
}

/// `diag_ui_categories`.
pub(crate) fn ui_categories() -> bool {
    current().ui_categories
}

/// Whether `diag_frame` currently removes `bit` (a [`FrameSkip`] constant).
pub(crate) fn frame_skips(bit: u32) -> bool {
    current().frame & bit != 0
}

/// Reads every switch now.
fn read() -> TerrainDiag {
    read_with(&value)
}

/// Builds the switches from `lookup`, which maps an upper-case switch name to its value.
fn read_with(lookup: &dyn Fn(&str) -> Option<String>) -> TerrainDiag {
    let value = lookup;
    let number = |name: &str| {
        value(name)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    let flag = |name: &str| number(name) != 0;
    TerrainDiag {
        solid_trivial: flag("SOLID_TRIVIAL"),
        cutout_as_solid: flag("CUTOUT_AS_SOLID"),
        solid_first: flag("SOLID_FIRST"),
        front_to_back: flag("FRONT_TO_BACK"),
        skip_streams: (number("SKIP_STREAMS") & 15) as u8,
        no_split: flag("NO_SPLIT"),
        vertex: number("VERTEX").min(6) as u8,
        max_chunks: number("MAX_CHUNKS"),
        keep_stride: number("KEEP_STRIDE"),
        sample: number("SAMPLE").min(4) as u8,
        page0: flag("PAGE0"),
        shade: number("SHADE").min(10) as u8,
        vary: number("VARY").min(4) as u8,
        flat_units: flag("FLAT_UNITS"),
        frame: number("FRAME"),
        sky: (number("SKY") & 3) as u8,
        ui_categories: flag("UI_CATEGORIES"),
        units_off: value("UNITS").is_some_and(|v| v.trim() == "0"),
        counts: flag("COUNTS"),
    }
}

/// Whether this frame logs counts (every 120 frames while `diag_counts` is on).
pub(crate) fn count_frame() -> bool {
    current().counts && FRAME.load(Ordering::Relaxed) % 120 == 1
}

/// Whether the section at chunk-grid position `section` is drawn under `diag_max_chunks` and
/// `diag_keep_stride`, with the camera at block position `camera`.
pub(in crate::chunk) fn keeps_section(
    diag: &TerrainDiag,
    origin: [i32; 3],
    camera: Option<[f64; 3]>,
) -> bool {
    let section = origin.map(|v| v.div_euclid(16));
    if diag.keep_stride > 1 {
        let hash = (section[0] as u32)
            .wrapping_mul(0x9e37_79b1)
            .wrapping_add((section[1] as u32).wrapping_mul(0x85eb_ca77))
            .wrapping_add((section[2] as u32).wrapping_mul(0xc2b2_ae3d));
        if !(hash >> 8).is_multiple_of(diag.keep_stride) {
            return false;
        }
    }
    if diag.max_chunks > 0
        && let Some(camera) = camera
    {
        let cx = (camera[0].floor() as i32).div_euclid(16);
        let cz = (camera[2].floor() as i32).div_euclid(16);
        let far = (section[0] - cx)
            .unsigned_abs()
            .max((section[2] - cz).unsigned_abs());
        if far > diag.max_chunks {
            return false;
        }
    }
    true
}

/// Squared distance from `camera` to the centre of the section at block `origin`.
pub(in crate::chunk) fn distance_squared(origin: [i32; 3], camera: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| f64::from(origin[axis]) + 8.0 - camera[axis])
        .map(|delta| delta * delta)
        .sum()
}

const SOLID_TRIVIAL: u32 = 1;
const CUTOUT_AS_SOLID: u32 = 2;
const VERTEX_MIN: u32 = 4;
const VERTEX_CULL: u32 = 8;
const VERTEX_NO_BLEND: u32 = 16;
/// Fragment shader defines selected by `diag_sample=1..=4`.
const SAMPLE_DEFINES: [&str; 4] = [
    "DIAG_SAMPLE_LEVEL",
    "DIAG_SAMPLE_IMPLICIT",
    "DIAG_SAMPLE_LOD0",
    "DIAG_SAMPLE_LOD_MAX",
];
/// The first of the four `diag_sample` bits.
const SAMPLE_SHIFT: u32 = 5;
const PAGE0: u32 = 1 << 9;
/// `diag_shade` occupies four bits from here.
const SHADE_SHIFT: u32 = 10;
/// `diag_vary` occupies three bits from here.
const VARY_SHIFT: u32 = 14;
/// `diag_vertex=4`: the fragment drops the per-fragment biome blend too.
const FRAGMENT_NO_BLEND: u32 = 1 << 17;
const FLAT_UNITS: u32 = 1 << 18;
const VERTEX_NO_QUAD_LOOKUPS: u32 = 1 << 19;
const VERTEX_NO_VARIATION_TINT: u32 = 1 << 20;
/// Vertex and fragment defines for each `diag_vary` value; the varying interface must agree.
fn vary_defines(vary: u32) -> (&'static [&'static str], &'static [&'static str]) {
    match vary {
        1 => (&["DIAG_VARY_FULL_VS_FLAT"], &["DIAG_FS_CONSTANT_INPUTS"]),
        2 => (&["DIAG_VARY_MIN_VS"], &[]),
        3 => (&["DIAG_VARY_POSITION_ONLY"], &["DIAG_FS_CONSTANT_INPUTS"]),
        4 => (&["DIAG_PACKED_VARYINGS"], &["DIAG_PACKED_VARYINGS"]),
        _ => (&[], &[]),
    }
}
/// The fragment shader defines for each `diag_shade` value.
fn shade_defines(shade: u32) -> &'static [&'static str] {
    match shade {
        1 => &["DIAG_SHADE_NO_LIGHTMAP"],
        2 => &["DIAG_SHADE_NO_FOG"],
        3 => &["DIAG_SHADE_NO_GAMMA"],
        4 => &["DIAG_SHADE_NO_TINT"],
        5 => &["DIAG_SHADE_ONE_LIGHT_READ"],
        6 => &["DIAG_SHADE_NO_POINT_LIGHT"],
        7 => &["DIAG_SHADE_VARYINGS"],
        8 => &[
            "DIAG_SHADE_NO_LIGHTMAP",
            "DIAG_SHADE_NO_FOG",
            "DIAG_SHADE_NO_GAMMA",
            "DIAG_SHADE_NO_TINT",
        ],
        9 => &["DIAG_SHADE_NO_BLEND"],
        10 => &["DIAG_SHADE_NO_MASKS"],
        _ => &[],
    }
}

/// The pipeline-changing switches as a variant key; zero leaves pipelines unchanged.
pub(in crate::chunk) fn pipeline_bits(diag: &TerrainDiag) -> u32 {
    let mut bits = 0;
    if diag.solid_trivial {
        bits |= SOLID_TRIVIAL;
    }
    if diag.cutout_as_solid {
        bits |= CUTOUT_AS_SOLID;
    }
    match diag.vertex {
        1 => bits |= VERTEX_MIN,
        2 => bits |= VERTEX_CULL,
        3 => bits |= VERTEX_NO_BLEND,
        4 => bits |= VERTEX_NO_BLEND | FRAGMENT_NO_BLEND,
        5 => bits |= VERTEX_NO_QUAD_LOOKUPS,
        6 => bits |= VERTEX_NO_VARIATION_TINT,
        _ => {}
    }
    if diag.sample > 0 {
        bits |= 1 << (SAMPLE_SHIFT + u32::from(diag.sample) - 1);
    }
    if diag.page0 {
        bits |= PAGE0;
    }
    if diag.flat_units {
        bits |= FLAT_UNITS;
    }
    bits | u32::from(diag.shade) << SHADE_SHIFT | u32::from(diag.vary) << VARY_SHIFT
}

/// Each render app's diagnostic variants by base pipeline and switch bits. Pipeline ids are only
/// meaningful within one app's cache, so the map lives in that app's queue system.
pub(in crate::chunk) type Variants = HashMap<(CachedRenderPipelineId, u32), CachedRenderPipelineId>;

/// The diagnostic variant of the ready pipeline `id`, queued on first use. Returns `id` while
/// `id` is still compiling or when no switch applies to it.
pub(in crate::chunk) fn pipeline(
    cache: &PipelineCache,
    variants: &mut Variants,
    id: CachedRenderPipelineId,
    bits: u32,
) -> CachedRenderPipelineId {
    if bits == 0 || cache.get_render_pipeline(id).is_none() {
        return id;
    }
    *variants.entry((id, bits)).or_insert_with(|| {
        let mut descriptor = cache.get_render_pipeline_descriptor(id).clone();
        if apply(&mut descriptor, bits) {
            println!(
                "CINNABAR_TERRAIN_DIAG queued variant {bits:#x} of {:?}",
                descriptor.label
            );
            cache.queue_render_pipeline(descriptor)
        } else {
            id
        }
    })
}

/// Applies `bits` to a terrain pipeline; `false` when none applies to it.
fn apply(descriptor: &mut RenderPipelineDescriptor, bits: u32) -> bool {
    let mut changed = false;
    let chunk_shader = descriptor.vertex.shader == super::CHUNK_SHADER_HANDLE;
    if let Some(fragment) = descriptor.fragment.as_mut()
        && fragment.shader == super::CHUNK_SHADER_HANDLE
    {
        let cutout = fragment.entry_point.as_deref() == Some("fragment");
        let solid = fragment.entry_point.as_deref() == Some("fragment_solid");
        if bits & CUTOUT_AS_SOLID != 0 && cutout {
            fragment.entry_point = Some("fragment_solid".into());
            changed = true;
        }
        if bits & SOLID_TRIVIAL != 0 && solid {
            fragment.shader_defs.push("OPAQUE_OVERDRAW".into());
            changed = true;
        }
        for (index, define) in SAMPLE_DEFINES.into_iter().enumerate() {
            if bits & (1 << (SAMPLE_SHIFT + index as u32)) != 0 {
                fragment.shader_defs.push(define.into());
                changed = true;
            }
        }
        if bits & PAGE0 != 0 {
            fragment.shader_defs.push("DIAG_PAGE0".into());
            changed = true;
        }
        if bits & FRAGMENT_NO_BLEND != 0 {
            fragment.shader_defs.push("DIAG_SHADE_NO_BLEND".into());
            changed = true;
        }
        for define in shade_defines(bits >> SHADE_SHIFT & 15) {
            fragment.shader_defs.push((*define).into());
            changed = true;
        }
    }
    if chunk_shader && descriptor.vertex.entry_point.as_deref() == Some("vertex_unit") {
        if bits & VERTEX_MIN != 0 {
            descriptor.vertex.shader_defs.push("DIAG_VERTEX_MIN".into());
            changed = true;
        }
        if bits & VERTEX_CULL != 0 {
            descriptor
                .vertex
                .shader_defs
                .push("DIAG_VERTEX_CULL".into());
            changed = true;
        }
        if bits & VERTEX_NO_VARIATION_TINT != 0 {
            descriptor
                .vertex
                .shader_defs
                .push("DIAG_NO_VARIATION_TINT".into());
            changed = true;
        }
        if bits & VERTEX_NO_QUAD_LOOKUPS != 0 {
            descriptor
                .vertex
                .shader_defs
                .push("DIAG_NO_QUAD_LOOKUPS".into());
            changed = true;
        }
        if bits & FLAT_UNITS != 0 {
            descriptor.vertex.shader_defs.push("DIAG_FLAT_UNITS".into());
            changed = true;
        }
        if bits & VERTEX_NO_BLEND != 0 {
            descriptor
                .vertex
                .shader_defs
                .push("DIAG_NO_VERTEX_BLEND".into());
            changed = true;
        }
    }
    let (vertex_defines, fragment_defines) = vary_defines(bits >> VARY_SHIFT & 7);
    let enhanced = descriptor.vertex.shader_defs.iter().any(
        |define| matches!(define, bevy::shader::ShaderDefVal::Bool(name, true) if name.starts_with("ENHANCED")),
    );
    if !vertex_defines.is_empty()
        && !enhanced
        && chunk_shader
        && descriptor.vertex.entry_point.as_deref() == Some("vertex_unit")
        && let Some(fragment) = descriptor.fragment.as_mut()
        && fragment.shader == super::CHUNK_SHADER_HANDLE
    {
        for define in vertex_defines {
            descriptor.vertex.shader_defs.push((*define).into());
        }
        for define in fragment_defines {
            fragment.shader_defs.push((*define).into());
        }
        changed = true;
    }
    if changed {
        descriptor.label = Some(
            format!(
                "{} (diagnostic {bits:#x})",
                descriptor.label.as_deref().unwrap_or("terrain")
            )
            .into(),
        );
    }
    changed
}

/// Work submitted for one opaque stream in a counted frame.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::chunk) struct StreamCounts {
    pub(in crate::chunk) sections: u64,
    pub(in crate::chunk) draws: u64,
    pub(in crate::chunk) instances: u64,
    pub(in crate::chunk) units: u64,
    pub(in crate::chunk) padded_quads: u64,
}

/// Logs one counted frame of the batched unit path.
pub(in crate::chunk) fn log_unit_counts(streams: &[StreamCounts; 4], occluder_units: u64) {
    let index_count = u64::from(super::pipeline::units::index_count());
    let unique = index_count / 6 * 4;
    let mut totals = (0, 0, 0, 0);
    for (stream, counts) in streams.iter().enumerate() {
        let quads = counts.units * u64::from(render_model::terrain_units::QUADS_PER_UNIT)
            - counts
                .padded_quads
                .min(counts.units * u64::from(render_model::terrain_units::QUADS_PER_UNIT));
        println!(
            "CINNABAR_TERRAIN_COUNTS stream={stream} sections={} draws={} instances={} units={} quads={quads} padded_quads={} indices={} vertex_invocations_max={}",
            counts.sections,
            counts.draws,
            counts.instances,
            counts.units,
            counts.padded_quads,
            counts.instances * index_count,
            counts.instances * unique,
        );
        totals.0 += counts.draws;
        totals.1 += counts.instances * index_count;
        totals.2 += counts.instances * unique;
        totals.3 += quads;
    }
    println!(
        "CINNABAR_TERRAIN_COUNTS total draws={} indices={} vertex_invocations_max={} quads={} occluder_units={occluder_units}",
        totals.0,
        totals.1,
        totals.2,
        totals.3
    );
}

/// The `debug.cinnabar.diag_*` property or `CINNABAR_DIAG_*` variable named `name`.
fn value(name: &str) -> Option<String> {
    let value = std::env::var(format!("CINNABAR_DIAG_{name}")).ok();
    #[cfg(target_os = "android")]
    let value = value.or_else(|| {
        system_property(&format!(
            "debug.cinnabar.diag_{}",
            name.to_ascii_lowercase()
        ))
    });
    value.filter(|value| !value.is_empty())
}

/// Reads an Android system property; `None` when unset, empty or not UTF-8.
#[cfg(target_os = "android")]
fn system_property(name: &str) -> Option<String> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut value = [0_u8; libc::PROP_VALUE_MAX as usize];
    // SAFETY: `name` is NUL-terminated and `value` holds PROP_VALUE_MAX bytes, as required.
    let length = unsafe { libc::__system_property_get(name.as_ptr(), value.as_mut_ptr().cast()) };
    let length = usize::try_from(length).ok().filter(|&length| length > 0)?;
    String::from_utf8(value[..length.min(value.len())].to_vec()).ok()
}
