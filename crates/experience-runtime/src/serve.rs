//! The stdio session of one Experience. The adapter's first frame loads it; each callback frame
//! then gets a result with the same `seq`, until `shutdown` or the end of input. Only frames go
//! to the output; logs go to stderr.

use std::fmt::Display;
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;

use anyhow::Result;
use wasmtime::Engine;

use crate::callback;
use crate::load::{self, EpochTicker, Loaded};
use crate::manifest::Manifest;
use crate::protocol::{
    BlockDef, Call, FailKind, ItemDef, Outcome, PROTOCOL_VERSION, Recipe, Request, Response,
    ServerItem, bounded_reason, read_frame, write_frame,
};

/// The session ended with `shutdown`, or with the end of input between frames.
pub const EXIT_OK: i32 = 0;
/// The artifact did not load, and the adapter was answered `load_failed`.
pub const EXIT_LOAD_FAILED: i32 = 1;
/// A frame did not decode or could not be written, or was not the request expected at its point
/// in the session.
pub const EXIT_PROTOCOL: i32 = 2;

/// Why a result that is too large for a frame failed instead.
const OVERSIZED_RESULT: &str = "result exceeds the frame limit";
/// Why a load failed whose `loaded` answer is too large for a frame.
const OVERSIZED_LOADED: &str = "the loaded answer exceeds the frame limit";
/// Why a load failed whose `load_failed` answer is too large for a frame.
const OVERSIZED_LOAD_FAILED: &str = "the load failure exceeds the frame limit";

/// Serves one session, reading requests from `input` and writing responses to `output`, and
/// returns the process exit code. With `report_fuel`, each callback logs the fuel it consumed.
/// An error is a frame that could not be written; the binary then exits with [`EXIT_PROTOCOL`].
#[expect(
    clippy::print_stderr,
    reason = "`fuel <id> <call> <n>` lines on stderr are the --report-fuel interface"
)]
pub fn serve(mut input: impl Read, mut output: impl Write, report_fuel: bool) -> Result<i32> {
    let (dir, items, recipes) = match read_frame(&mut input) {
        Ok(Some(Request::Load {
            dir,
            items,
            recipes,
        })) => (dir, items, recipes),
        Ok(Some(Request::Callback { .. } | Request::Shutdown {})) => {
            return Ok(protocol_error("the first frame is not load"));
        }
        Ok(None) => return Ok(EXIT_OK),
        Err(error) => return Ok(protocol_error(error)),
    };
    // The ticker drives every deadline, so it lives as long as the session.
    let (engine, _ticker, loaded) = match start(Path::new(&dir), items, recipes) {
        Ok(started) => started,
        Err(error) => {
            answer_load_failed(&mut output, format!("{error:#}"))?;
            return Ok(EXIT_LOAD_FAILED);
        }
    };
    if !answer_loaded(
        &mut output,
        &loaded.manifest,
        &loaded.blocks,
        &loaded.items,
        &loaded.entities,
        loaded.focus,
        loaded.worlds.clone(),
        loaded.overlays.clone(),
    )? {
        return Ok(EXIT_LOAD_FAILED);
    }
    loop {
        let request = match read_frame(&mut input) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(EXIT_OK),
            Err(error) => return Ok(protocol_error(error)),
        };
        let (seq, call) = match &request {
            Request::Callback { seq, call, .. } => (*seq, call),
            Request::Shutdown {} => return Ok(EXIT_OK),
            Request::Load { .. } => return Ok(protocol_error("load after the artifact loaded")),
        };
        let (outcome, fuel) = callback::run_metered(&engine, &loaded, &request);
        if report_fuel {
            eprintln!("fuel {} {} {fuel}", loaded.manifest.id, call_kind(call));
        }
        answer(&mut output, seq, outcome)?;
    }
}

/// The engine with its ticker, and the artifact in `dir` loaded on it, which may make stacks of
/// the server's `items` and match grids against its `recipes`.
fn start(
    dir: &Path,
    items: Vec<ServerItem>,
    recipes: Vec<Recipe>,
) -> Result<(Engine, EpochTicker, Loaded)> {
    let (engine, ticker) = load::engine()?;
    let loaded = load::load(&engine, dir)?
        .with_server_items(items)?
        .with_recipes(recipes)?;
    Ok((engine, ticker, loaded))
}

/// Answers a load with what loaded: the manifest's id and version, the blocks and items, whether
/// its world takes a `focus`, and its worlds and overlays. An answer too large for a frame fails
/// the load instead. Returns whether the load stands.
#[expect(
    clippy::too_many_arguments,
    reason = "the loaded response carries independently versioned registration lists"
)]
fn answer_loaded(
    output: &mut impl Write,
    manifest: &Manifest,
    blocks: &[BlockDef],
    items: &[ItemDef],
    entities: &[crate::protocol::EntityDefinition],
    focus: bool,
    worlds: Option<crate::protocol::WorldRegistration>,
    overlays: Option<crate::protocol::OverlayRegistration>,
) -> io::Result<bool> {
    let api = crate::host::Api::of(&manifest.api);
    let response = Response::Loaded {
        entities: entities.to_vec(),
        simulation: api.is_some_and(crate::host::Api::simulation),
        protocol: PROTOCOL_VERSION,
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        blocks: blocks.to_vec(),
        items: items.to_vec(),
        focus,
        worlds,
        overlays,
    };
    let failed = || Response::LoadFailed {
        reason: OVERSIZED_LOADED.to_owned(),
    };
    write_or(output, &response, failed)
}

/// Answers a failed load with `reason` cut by [`bounded_reason`], and logs the same.
#[expect(
    clippy::print_stderr,
    reason = "the helper's stderr is the log stream its supervisor captures"
)]
fn answer_load_failed(output: &mut impl Write, reason: String) -> io::Result<()> {
    let reason = bounded_reason(reason);
    eprintln!("serve: load failed: {reason}");
    let fixed = || Response::LoadFailed {
        reason: OVERSIZED_LOAD_FAILED.to_owned(),
    };
    write_or(output, &Response::LoadFailed { reason }, fixed)?;
    Ok(())
}

/// Logs why the session ends without an answer, and returns [`EXIT_PROTOCOL`].
#[expect(
    clippy::print_stderr,
    reason = "the helper's stderr is the log stream its supervisor captures"
)]
fn protocol_error(error: impl Display) -> i32 {
    eprintln!("serve: protocol error: {error}");
    EXIT_PROTOCOL
}

/// Writes the result of callback `seq`. A result too large for a frame fails as a limit
/// instead, so the callback is still answered.
fn answer(output: &mut impl Write, seq: u64, outcome: Outcome) -> io::Result<()> {
    let failed = || Response::Result {
        seq,
        outcome: Outcome::Failed {
            kind: FailKind::Limit,
            reason: OVERSIZED_RESULT.to_owned(),
        },
    };
    write_or(output, &Response::Result { seq, outcome }, failed)?;
    Ok(())
}

/// Writes `response`, or `fallback()` when `response` is too large for a frame, which
/// [`write_frame`] refuses before it writes anything. Returns whether `response` was written.
#[expect(
    clippy::print_stderr,
    reason = "the helper's stderr is the log stream its supervisor captures"
)]
fn write_or(
    output: &mut impl Write,
    response: &Response,
    fallback: impl FnOnce() -> Response,
) -> io::Result<bool> {
    match write_frame(output, response) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            eprintln!("serve: answer refused: {error}");
            write_frame(output, &fallback())?;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// The `type` of `call` in the protocol.
fn call_kind(call: &Call) -> &'static str {
    match call {
        Call::Entity { .. } => "entity",
        Call::Tick { .. } => "tick",
        Call::Timer { .. } => "timer",
        Call::Player { .. } => "player",
        Call::Place { .. } => "place",
        Call::Break { .. } => "break",
        Call::Interact { .. } => "interact",
        Call::Neighbor { .. } => "neighbor",
        Call::ClientMessage { .. } => "client_message",
        Call::Epoch { .. } => "epoch",
        Call::ScheduledUpdate { .. } => "scheduled_update",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{OVERSIZED_LOADED, OVERSIZED_RESULT, answer, answer_loaded};
    use crate::limits::{MAX_BLOCKS, MAX_FRAME_BYTES, MAX_ITEMS, MAX_OVERLAY_TYPES};
    use crate::manifest::Manifest;
    use crate::protocol::{
        BlockDef, BlockDrops, BlockItem, BlockState, BoneVisibility, FailKind, ItemDef, Material,
        Mining, Op, Outcome, OverlayDef, OverlayRegistration, RenderMethod, Response, StateDef,
        StateTest, StateValue, StateValues, Visual, read_frame,
    };

    fn manifest() -> Manifest {
        Manifest {
            id: "probe".to_owned(),
            version: "0.1.0".to_owned(),
            api: "0.1".to_owned(),
            data_schema: 1,
            files: BTreeMap::new(),
        }
    }

    /// A `loaded` answer too large for a frame fails the load instead, and nothing of it is
    /// written.
    #[test]
    fn oversized_loaded_fails_the_load() {
        let block = BlockDef {
            id: "probe:counter".to_owned(),
            display_name: "x".repeat(MAX_FRAME_BYTES),
            textures: Vec::new(),
            mining: Mining::Unbreakable {},
            states: Vec::new(),
            placement: Vec::new(),
            visual: None,
            permutations: Vec::new(),
            network: false,
            drops: BlockDrops::Itself,
            item: BlockItem::Listed,
        };
        let mut output = Vec::new();
        assert!(
            !answer_loaded(
                &mut output,
                &manifest(),
                &[block],
                &[],
                &[],
                false,
                None,
                None
            )
            .unwrap()
        );
        let mut frames = output.as_slice();
        let failed = Response::LoadFailed {
            reason: OVERSIZED_LOADED.to_owned(),
        };
        assert_eq!(read_frame(&mut frames).unwrap(), Some(failed));
        assert_eq!(read_frame::<Response>(&mut frames).unwrap(), None);
    }

    /// The `loaded` answer carries every definition at once: `MAX_BLOCKS` blocks shaped like
    /// AE2's covered cable, which took 38 KB of AE2's 569 KB answer (six four-way face states, two
    /// materials, sixteen bones shown under 384 state tests, every path absolute below a 200-byte
    /// install directory), `MAX_OVERLAY_TYPES` overlay types of the same shape and `MAX_ITEMS`
    /// items still fit one frame.
    #[test]
    fn loaded_answer_of_ae2_sized_blocks_at_the_bounds_fits_a_frame() {
        let root = format!("/{}", "d".repeat(199));
        let faces = ["down", "up", "north", "south", "west", "east"];
        let state = |face: &str| format!("probe:{face}");
        let test = |face: &str, kind: &str| StateTest {
            state: BlockState {
                name: state(face),
                value: StateValue::Choice(kind.to_owned()),
            },
            equal: kind != "none",
        };
        let bones: Vec<BoneVisibility> = (0..16)
            .map(|bone| BoneVisibility {
                bone: format!("cap_{bone}"),
                visible: faces
                    .iter()
                    .map(|face| {
                        let other = faces[bone % faces.len()];
                        vec![
                            test(face, "machine"),
                            test(face, "covered"),
                            test(other, "none"),
                            test(other, "glass"),
                        ]
                    })
                    .collect(),
            })
            .collect();
        let blocks: Vec<BlockDef> = (0..MAX_BLOCKS)
            .map(|i| BlockDef {
                id: format!("probe:cable_{i}"),
                display_name: "Covered Cable".to_owned(),
                textures: Vec::new(),
                mining: Mining::Unbreakable {},
                states: faces
                    .iter()
                    .map(|face| StateDef {
                        name: state(face),
                        values: StateValues::Choices(
                            ["none", "glass", "covered", "machine"]
                                .map(str::to_owned)
                                .to_vec(),
                        ),
                    })
                    .collect(),
                placement: Vec::new(),
                visual: Some(Visual {
                    geometry: Some(format!("{root}/assets/models/cable_{i}.geo.json")),
                    materials: ["base", "core"]
                        .map(|instance| Material {
                            instance: instance.to_owned(),
                            path: format!("{root}/assets/textures/cable/{instance}_{i}.png"),
                            render_method: RenderMethod::AlphaTest,
                            flipbook: None,
                        })
                        .to_vec(),
                    bones: bones.clone(),
                    collision: None,
                    selection: None,
                    rotation: None,
                }),
                permutations: Vec::new(),
                network: true,
                drops: BlockDrops::Nothing,
                item: BlockItem::Unlisted,
            })
            .collect();
        let items: Vec<ItemDef> = (0..MAX_ITEMS)
            .map(|i| ItemDef {
                id: format!("probe:cell_{i}"),
                display_name: "Storage Cell".to_owned(),
                icon: format!("{root}/assets/textures/item/cell_{i}.png"),
                max_stack: 64,
                listed: false,
            })
            .collect();
        let overlays = OverlayRegistration {
            hosts: blocks.iter().map(|block| block.id.clone()).collect(),
            types: (0..MAX_OVERLAY_TYPES)
                .map(|i| OverlayDef {
                    id: format!("probe:cable_arm_{i}"),
                    states: blocks[0].states.clone(),
                    visual: blocks[0].visual.clone().expect("the cable has a visual"),
                    permutations: Vec::new(),
                })
                .collect(),
        };
        let mut output = Vec::new();
        let answered = answer_loaded(
            &mut output,
            &manifest(),
            &blocks,
            &items,
            &[],
            false,
            None,
            Some(overlays.clone()),
        );
        assert!(answered.unwrap());
        let mut frames = output.as_slice();
        let Some(Response::Loaded {
            blocks: read,
            overlays: read_overlays,
            ..
        }) = read_frame(&mut frames).unwrap()
        else {
            panic!("no loaded answer");
        };
        assert_eq!(read, blocks);
        assert_eq!(read_overlays, Some(overlays));
    }

    /// A result too large for a frame is answered as a `limit` failure instead, and nothing of
    /// it is written.
    #[test]
    fn oversized_result_fails_with_limit() {
        let tell = Op::Tell {
            player: String::new(),
            text: "x".repeat(MAX_FRAME_BYTES),
        };
        let mut output = Vec::new();
        answer(&mut output, 7, Outcome::Committed { ops: vec![tell] }).unwrap();
        let mut frames = output.as_slice();
        let failed = Response::Result {
            seq: 7,
            outcome: Outcome::Failed {
                kind: FailKind::Limit,
                reason: OVERSIZED_RESULT.to_owned(),
            },
        };
        assert_eq!(read_frame(&mut frames).unwrap(), Some(failed));
        assert_eq!(read_frame::<Response>(&mut frames).unwrap(), None);
    }
}
