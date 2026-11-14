//! Opt-in opaque terrain diagnostics for phone GPU investigations. Each switch is read about
//! once a second from the `debug.cinnabar.diag_*` Android system property (`adb shell setprop`)
//! or the matching `CINNABAR_DIAG_*` environment variable, so it can be flipped while the client
//! runs. Every switch is off by default; its state is logged whenever it changes.

use std::sync::{
    Mutex, RwLock,
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
    /// `diag_vertex=3`: the full vertex shader without the per-vertex mixed biome tint blend.
    pub(in crate::chunk) vertex: u8,
    /// (g) `diag_max_chunks=N`: draws only sections within N chunks (horizontal) of the camera.
    pub(in crate::chunk) max_chunks: u32,
    /// (g') `diag_keep_stride=K`: draws only about one section in K.
    pub(in crate::chunk) keep_stride: u32,
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
    units_off: false,
    counts: false,
});
static FRAME: AtomicU32 = AtomicU32::new(0);

/// The switch values as of the last refresh.
pub(in crate::chunk) fn current() -> TerrainDiag {
    *STATE.read().unwrap_or_else(|poison| poison.into_inner())
}

/// Re-reads the switches every 60 calls (and on the first), logging any change.
pub(in crate::chunk) fn refresh() {
    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
    if frame % 60 != 0 {
        return;
    }
    let number = |name: &str| {
        value(name)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    let flag = |name: &str| number(name) != 0;
    let next = TerrainDiag {
        solid_trivial: flag("SOLID_TRIVIAL"),
        cutout_as_solid: flag("CUTOUT_AS_SOLID"),
        solid_first: flag("SOLID_FIRST"),
        front_to_back: flag("FRONT_TO_BACK"),
        skip_streams: (number("SKIP_STREAMS") & 15) as u8,
        no_split: flag("NO_SPLIT"),
        vertex: number("VERTEX").min(3) as u8,
        max_chunks: number("MAX_CHUNKS"),
        keep_stride: number("KEEP_STRIDE"),
        units_off: value("UNITS").is_some_and(|v| v.trim() == "0"),
        counts: flag("COUNTS"),
    };
    let mut state = STATE.write().unwrap_or_else(|poison| poison.into_inner());
    if frame == 0 || *state != next {
        println!("CINNABAR_TERRAIN_DIAG {next:?}");
        *state = next;
    }
}

/// Whether this frame logs counts (every 120 frames while `diag_counts` is on).
pub(in crate::chunk) fn count_frame() -> bool {
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
        if (hash >> 8) % diag.keep_stride != 0 {
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

const SOLID_TRIVIAL: u8 = 1;
const CUTOUT_AS_SOLID: u8 = 2;
const VERTEX_MIN: u8 = 4;
const VERTEX_CULL: u8 = 8;
const VERTEX_NO_BLEND: u8 = 16;

/// The pipeline-changing switches as a variant key; zero leaves pipelines unchanged.
pub(in crate::chunk) fn pipeline_bits(diag: &TerrainDiag) -> u8 {
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
        _ => {}
    }
    bits
}

static VARIANTS: Mutex<Option<HashMap<(CachedRenderPipelineId, u8), CachedRenderPipelineId>>> =
    Mutex::new(None);

/// The diagnostic variant of the ready pipeline `id`, queued on first use. Returns `id` while
/// `id` is still compiling or when no switch applies to it.
pub(in crate::chunk) fn pipeline(
    cache: &PipelineCache,
    id: CachedRenderPipelineId,
    bits: u8,
) -> CachedRenderPipelineId {
    if bits == 0 || cache.get_render_pipeline(id).is_none() {
        return id;
    }
    let mut variants = VARIANTS.lock().unwrap_or_else(|poison| poison.into_inner());
    let variants = variants.get_or_insert_with(HashMap::default);
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
fn apply(descriptor: &mut RenderPipelineDescriptor, bits: u8) -> bool {
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
        if bits & VERTEX_NO_BLEND != 0 {
            descriptor
                .vertex
                .shader_defs
                .push("DIAG_NO_VERTEX_BLEND".into());
            changed = true;
        }
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
