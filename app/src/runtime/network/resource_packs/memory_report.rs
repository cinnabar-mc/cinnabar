//! Env-gated pack compile memory report: admits `CINNABAR_JOIN_PACKS` with the client's
//! device subpack selection, compiles it through the join's subscriber path, one subscriber at
//! a time and then all together, and prints each stage's resident high-water mark and retained
//! growth. `CINNABAR_MEMORY_ROUNDS` repeats the stages, as rejoins do, to expose retention.
//! Run: `CINNABAR_JOIN_PACKS=<a.mcpack:b.mcpack> CINNABAR_JOIN_CARRIERS=<compiled> cargo test -p
//! cinnabar --features reports --lib pack_compile_memory_report -- --nocapture`.

use super::fingerprint_bench::{install_join_carriers, join_fixture, join_handoff};
use super::*;
use pack_runtime::reload_diff::Changes;

/// One `/proc/self/status` field in MiB.
fn status_mib(field: &str) -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|rest| {
            rest.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<f64>()
                .ok()
        })
        .unwrap_or(0.0)
        / 1024.0
}

/// Resident growth since the window opened; opening resets the high-water mark.
struct Window {
    before: f64,
    started: std::time::Instant,
}

impl Window {
    fn open() -> Self {
        let _ = std::fs::write("/proc/self/clear_refs", "5");
        Self {
            before: status_mib("VmRSS:"),
            started: std::time::Instant::now(),
        }
    }

    /// Highest resident size since opening, above the opening size.
    fn peak(&self) -> f64 {
        status_mib("VmHWM:") - self.before
    }

    /// Current resident size above the opening size.
    fn retained(&self) -> f64 {
        status_mib("VmRSS:") - self.before
    }
}

/// Retained entity pixels and the encoded catalog size, in MiB.
fn entity_bytes(pack: Option<&assets::SessionEntityPack>) -> String {
    let mib = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
    pack.map_or_else(String::new, |pack| {
        format!(
            "entity_textures={} entity_texture_mib={:.0} equipment_texture_mib={:.0} entity_blob_mib={:.1}",
            pack.textures.len(),
            mib(pack.textures.iter().map(|texture| texture.rgba8.len()).sum()),
            mib(pack.equipment.as_ref().map_or(0, |catalog| {
                catalog.textures().iter().map(|texture| texture.rgba8.len()).sum()
            })),
            mib(pack.assets.encode().map_or(0, |blob| blob.len())),
        )
    })
}

/// Physical memory of the reported host: `CINNABAR_MEMORY_HOST_GIB` (default 16) GiB.
fn host_bytes() -> u64 {
    std::env::var("CINNABAR_MEMORY_HOST_GIB")
        .ok()
        .and_then(|gib| gib.parse::<u64>().ok())
        .unwrap_or(16)
        << 30
}

/// Installs the device limits a desktop GPU reports, with the terrain budget of the reported
/// host, as renderer startup does before a join.
fn install_device() {
    const MAX_DIMENSION_2D: u32 = 16_384;
    const MAX_ARRAY_LAYERS: u32 = 2_048;
    assets::texture::policy::set_device_policy(MAX_DIMENSION_2D);
    assets::texture::policy::set_terrain_device_policy(
        MAX_DIMENSION_2D,
        resource_pack::subpacks::device_memory_tier(host_bytes()),
    );
    let _ = OVERLAY_TEXTURE_LAYER_LIMIT
        .get_or_init(std::sync::OnceLock::new)
        .set(MAX_ARRAY_LAYERS);
}

/// Icon keys a server registry could send: every item texture any layer names, as custom
/// items declaring that icon would request it.
fn declared_icon_keys(view: &LayeredPackView) -> Vec<(Arc<str>, Arc<str>)> {
    let mut keys = std::collections::BTreeSet::new();
    for bytes in view.read_layers("textures/item_texture.json") {
        let Some(root) = pack_runtime::pack_textures::parse_pack_json(&bytes) else {
            continue;
        };
        if let Some(data) = root.get("texture_data").and_then(|data| data.as_object()) {
            keys.extend(data.keys().cloned());
        }
    }
    keys.into_iter()
        .map(|key| (Arc::from(format!("bench:{key}")), Arc::from(key)))
        .collect()
}

/// Only `pick`'s subscribers compile; the rest keep their empty defaults.
fn only(pick: impl FnOnce(&mut Changes)) -> Changes {
    let mut changes = Changes::all();
    changes.contents = false;
    changes.blocks = false;
    changes.block_entity_art = false;
    changes.icons = false;
    changes.glyphs = false;
    changes.entities = false;
    changes.ui = false;
    changes.sounds = false;
    changes.language = false;
    changes.aim_assist = false;
    pick(&mut changes);
    changes
}

#[test]
fn pack_compile_memory_report() {
    let Some(paths) = join_fixture("pack_compile_memory_report") else {
        return;
    };
    install_device();
    let window = Window::open();
    install_join_carriers();
    eprintln!(
        "PACK_COMPILE_MEMORY carriers retained_mib={:.0}",
        window.retained()
    );
    let window = Window::open();
    let stack = resource_pack::validate_handoff_for_device(join_handoff(&paths), host_bytes());
    assert!(stack.rejections().is_empty(), "{:?}", stack.rejections());
    eprintln!(
        "PACK_COMPILE_MEMORY admission peak_mib={:.0} retained_mib={:.0} packs={}",
        window.peak(),
        window.retained(),
        stack.packs().len()
    );
    let inputs = Arc::new(pack_runtime::PackInputs {
        icons: declared_icon_keys(&LayeredPackView::new(Arc::clone(&stack))),
        ..Default::default()
    });
    eprintln!("PACK_COMPILE_MEMORY icon_keys={}", inputs.icons.len());
    let stages = std::env::var("CINNABAR_MEMORY_STAGES").unwrap_or_default();
    let selected = |label: &str| stages.is_empty() || stages.split(',').any(|stage| stage == label);
    let rounds = std::env::var("CINNABAR_MEMORY_ROUNDS")
        .ok()
        .and_then(|rounds| rounds.parse::<usize>().ok())
        .unwrap_or(1);
    let session = Window::open();
    for round in 1..=rounds {
        eprintln!("PACK_COMPILE_MEMORY round={round}");
        report_stages(&stack, &inputs, &selected);
        eprintln!(
            "PACK_COMPILE_MEMORY round={round} retained_since_first_mib={:.0}",
            session.retained()
        );
    }
}

/// Compiles the selected stages once, printing each one's peak and retained growth.
#[expect(
    clippy::print_stderr,
    reason = "the memory report prints PACK_COMPILE_MEMORY lines on stderr"
)]
fn report_stages(
    stack: &Arc<resource_pack::ValidatedPackStack>,
    inputs: &Arc<pack_runtime::PackInputs>,
    selected: &dyn Fn(&str) -> bool,
) {
    if selected("entities_only") {
        let window = Window::open();
        let view = LayeredPackView::tracked(Arc::clone(stack));
        let pack = pack_runtime::entity_pack::compile_session_entities(
            stack.fingerprint(),
            &view,
            &|| false,
        )
        .flatten();
        let compiled = window.peak();
        test_time::eventually_within(
            std::time::Duration::from_secs(600),
            "background entity pack stores",
            pack_runtime::entity_pack::stores_idle,
        );
        eprintln!(
            "PACK_COMPILE_MEMORY entities_only compile_peak_mib={compiled:.0} join_peak_mib={:.0} retained_mib={:.0} seconds={:.1} {}",
            window.peak(),
            window.retained(),
            window.started.elapsed().as_secs_f64(),
            entity_bytes(pack.as_deref()),
        );
    }
    if selected("artwork_only") {
        let window = Window::open();
        let artwork =
            super::super::entity_texture_reload::prepare(&LayeredPackView::new(Arc::clone(stack)));
        eprintln!(
            "PACK_COMPILE_MEMORY artwork_only peak_mib={:.0} retained_mib={:.0} pages={}",
            window.peak(),
            window.retained(),
            artwork.map_or(0, |pages| pages.pages().len()),
        );
    }
    for (label, changes) in [
        ("block_entity_art", only(|c| c.block_entity_art = true)),
        ("aim_assist", only(|c| c.aim_assist = true)),
        ("blocks+icons", only(|c| c.blocks = true)),
        ("icons", only(|c| c.icons = true)),
        ("language", only(|c| c.language = true)),
        ("glyphs", only(|c| c.glyphs = true)),
        ("entities+artwork", only(|c| c.entities = true)),
        ("ui", only(|c| c.ui = true)),
        ("sounds", only(|c| c.sounds = true)),
        ("all", Changes::all()),
    ] {
        if !selected(label) {
            continue;
        }
        let window = Window::open();
        let compile = || {
            compile_application(
                Arc::clone(stack),
                Arc::clone(inputs),
                None,
                changes,
                &|| false,
            )
            .unwrap()
        };
        let mut packs = chunk_pipeline::on_idle_world_cores(compile);
        let compiled = window.peak();
        let base = super::super::startup_actor_artwork();
        let stack_artwork = packs.entity_artwork.clone();
        let artwork = base.map(|base| entity_artwork_base(base, stack_artwork.as_deref()));
        packs.prepare_actor_artwork(artwork, None);
        let prepared = window.peak();
        test_time::eventually_within(
            std::time::Duration::from_secs(600),
            "background entity pack stores",
            pack_runtime::entity_pack::stores_idle,
        );
        eprintln!(
            "PACK_COMPILE_MEMORY {label} compile_peak_mib={compiled:.0} artwork_peak_mib={prepared:.0} join_peak_mib={:.0} retained_mib={:.0} seconds={:.1} icons={} {}",
            window.peak(),
            window.retained(),
            window.started.elapsed().as_secs_f64(),
            packs
                .item_icons
                .as_ref()
                .map_or(0, |icons| icons.icons.len()),
            entity_bytes(packs.entities.as_deref()),
        );
        drop(packs);
        eprintln!(
            "PACK_COMPILE_MEMORY {label} after_drop_mib={:.0}",
            window.retained()
        );
    }
}
