//! Server WIT 0.5's, 0.6's, 0.10's and 0.12's calls. x=24 makes each 0.5 call once; x=25 toggles
//! a lamp; x=26 marks every member of a node's network; x=27 makes stacks; x=28 asks the server's
//! recipes; x=29 schedules updates. Using a [`BUS`] adds or removes plates on it.

use experience_sdk::server::{
    BlockDef, BlockDrops, BlockItem, BlockPos, BlockRule, BlockState, BlockType, Callback,
    ContentRules, Face, GameMode, ItemDef, ItemKey, Material, Mining, NewStack, Overlay,
    OverlayLook, OverlayRegistration, OverlayType, OverlayVisual, Pixel, PixelBox, PlacementStates,
    Registration, RenderMethod, StateDef, StateValue, StateValues, TextureBinding, Visual,
    WorldError, cube,
};

/// A cube with the facing placement trait and one bool state, [`LAMP_ON`].
const LAMP: &str = "probe:lamp";
/// A cube that is a network member.
const NODE: &str = "probe:node";
/// The probe's item, one to a stack.
const CELL: &str = "probe:cell";
pub(crate) const LAMP_ON: &str = "probe:on";
/// A cube that carries overlays.
pub(crate) const BUS: &str = "probe:bus";
/// An overlay type: a two-pixel plate against its face, picked by its own box.
const PLATE: &str = "probe:plate";

/// The probe's blocks, each a cube showing `counter.png`: the counter `counter`, without states,
/// the lamp, with the facing trait and [`LAMP_ON`], the node, a network member, and the
/// [`BUS`]; and the item [`CELL`], one to a stack, with `counter.png` for an icon.
pub(crate) fn registration(counter: &str) -> Registration {
    let def = |id: &str, display_name: &str| BlockDef {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        textures: vec![TextureBinding {
            slot: "*".to_owned(),
            path: "counter.png".to_owned(),
        }],
        mining: Mining::Breakable(1.0),
    };
    let lamp = BlockType {
        states: vec![StateDef {
            name: LAMP_ON.to_owned(),
            values: StateValues::Bool,
        }],
        placement: PlacementStates::FACING_DIRECTION,
        ..cube(def(LAMP, "Probe Lamp"))
    };
    Registration {
        blocks: vec![
            cube(def(counter, "Probe Counter")),
            lamp,
            BlockType {
                network: true,
                ..cube(def(NODE, "Probe Node"))
            },
            cube(def(BUS, "Probe Bus")),
        ],
        items: vec![ItemDef {
            id: CELL.to_owned(),
            display_name: "Probe Cell".to_owned(),
            icon: "counter.png".to_owned(),
            max_stack: 1,
        }],
    }
}

/// The [`BUS`] carries overlays of the one type [`PLATE`], `plate.geo.json` drawn with
/// `counter.png`, whose selection box is the plate.
pub(crate) fn overlays() -> OverlayRegistration {
    let pixel = |x: f32, y: f32, z: f32| Pixel { x, y, z };
    OverlayRegistration {
        hosts: vec![BUS.to_owned()],
        types: vec![OverlayType {
            name: "plate".to_owned(),
            states: Vec::new(),
            visual: Visual {
                geometry: Some("plate.geo.json".to_owned()),
                materials: vec![Material {
                    instance: "*".to_owned(),
                    path: "counter.png".to_owned(),
                    render_method: RenderMethod::Opaque,
                    flipbook: None,
                }],
                bones: Vec::new(),
                collision: None,
                selection: Some(PixelBox {
                    min: pixel(0.0, 0.0, 0.0),
                    max: pixel(16.0, 16.0, 2.0),
                }),
                rotation: None,
            },
            permutations: Vec::new(),
        }],
    }
}

/// The [`BUS`] has no item and drops nothing by itself, as AE2's cable bus; the [`NODE`]'s item
/// and the [`CELL`] stay out of the creative inventory, and the node takes part in redstone.
pub(crate) fn content_rules() -> ContentRules {
    ContentRules {
        blocks: vec![
            BlockRule {
                block: BUS.to_owned(),
                drops: BlockDrops::Nothing,
                item: BlockItem::None,
                redstone: false,
            },
            BlockRule {
                block: NODE.to_owned(),
                drops: BlockDrops::Itself,
                item: BlockItem::Unlisted,
                redstone: true,
            },
        ],
        unlisted_items: vec![CELL.to_owned()],
    }
}

/// Using the bus at `p`: a click on one of its plates removes it, as `target {slot}`; else a
/// plate goes on the clicked face, in the slot of the face's index, as `plate {slot} {outcome}`.
pub(crate) fn use_bus(ctx: &Callback, p: BlockPos, face: Face, target: Option<u8>) -> String {
    let mut overlays = match ctx.overlays(p) {
        Ok(overlays) => overlays,
        Err(error) => return format!("overlays {}", error.name()),
    };
    if let Some(slot) = target {
        overlays.retain(|overlay| overlay.slot != slot);
        return format!("target {slot} {}", refusal(ctx.set_overlays(p, &overlays)));
    }
    let slot = face as u8;
    overlays.retain(|overlay| overlay.slot != slot);
    overlays.push(Overlay {
        slot,
        face,
        look: OverlayLook::Visual(OverlayVisual {
            kind: PLATE.to_owned(),
            states: Vec::new(),
        }),
    });
    format!("plate {slot} {}", refusal(ctx.set_overlays(p, &overlays)))
}

/// Places a lamp at `up`, the block above the interacted block `p`, reads its states, lights it
/// and reads them again, then makes each other 0.5 call once, as `<call> <states|ok|error>`
/// pairs short enough for one tell.
pub(crate) fn wit_0_5_calls(ctx: &Callback, p: BlockPos, up: BlockPos) -> String {
    let stack = || NewStack {
        id: "minecraft:stone".to_owned(),
        metadata: 0,
        count: 1,
        data: None,
    };
    // Its outcome shows in the states read after it.
    let _ = ctx.set_block(up, LAMP);
    let before = states(ctx.block_states(up));
    let lit = refusal(ctx.set_block_state(up, &[on(true)]));
    let after = states(ctx.block_states(up));
    format!(
        "states {before} set {lit} states {after} network {} inventory {} set-slot {} \
         drop-item {}",
        refusal(ctx.network()),
        refusal(ctx.inventory()),
        refusal(ctx.set_slot(0, Some(&stack()))),
        refusal(ctx.drop_item(p, &stack())),
    )
}

/// Turns the lamp at `p` on or off, keeping its data, and tells its states.
pub(crate) fn toggle_lamp(ctx: &Callback, p: BlockPos) -> String {
    let lit = ctx.block_states(p).map(|states| {
        states
            .iter()
            .any(|state| state.name == LAMP_ON && matches!(state.value, StateValue::Bool(true)))
    });
    match lit {
        Ok(lit) => match ctx.set_block_state(p, &[on(!lit)]) {
            Ok(()) => format!("lamp {}", states(ctx.block_states(p))),
            Err(error) => format!("error {}", error.name()),
        },
        Err(error) => format!("error {}", error.name()),
    }
}

/// Writes, to every member of the anchor's network, the member count as one byte of data, and
/// tells `network {members} truncated {bool} wrote {writes}`, or `network none` off a network.
pub(crate) fn mark_network(ctx: &Callback) -> String {
    match ctx.network() {
        Ok(Some(network)) => {
            let members = network.blocks.len();
            let mark = [members as u8];
            let wrote = network
                .blocks
                .iter()
                .filter(|pos| ctx.set_block_data(**pos, Some(&mark)).is_ok())
                .count();
            format!(
                "network {members} truncated {} wrote {wrote}",
                network.truncated
            )
        }
        Ok(None) => "network none".to_owned(),
        Err(error) => format!("error {}", error.name()),
    }
}

/// Reads the held stack, puts a cell with data `07` in slot 0 and drops two stone at `up`, and
/// tells `held {id} {count}/{max} plain {bool}|held none|error {name} cell {outcome} drop
/// {outcome}`.
pub(crate) fn make_stacks(ctx: &Callback, up: BlockPos) -> String {
    let held = match ctx.inventory() {
        Ok(inventory) => match &inventory.slots[usize::from(inventory.selected)] {
            Some(stack) => format!(
                "{} {}/{} plain {}",
                stack.id, stack.count, stack.max_count, stack.plain
            ),
            None => "none".to_owned(),
        },
        Err(error) => format!("error {}", error.name()),
    };
    let stack = |id: &str, count: u8, data: Option<Vec<u8>>| NewStack {
        id: id.to_owned(),
        metadata: 0,
        count,
        data,
    };
    let cell = refusal(ctx.set_slot(0, Some(&stack(CELL, 1, Some(vec![7])))));
    let drop = refusal(ctx.drop_item(up, &stack("minecraft:stone", 2, None)));
    format!("held {held} cell {cell} drop {drop}")
}

fn on(lit: bool) -> BlockState {
    BlockState {
        name: LAMP_ON.to_owned(),
        value: StateValue::Bool(lit),
    }
}

/// States as `name=value` joined by `,`, or the error's name.
fn states(result: Result<Vec<BlockState>, WorldError>) -> String {
    match result {
        Ok(states) => states
            .iter()
            .map(|state| match &state.value {
                StateValue::Bool(value) => format!("{}={value}", state.name),
                StateValue::Choice(value) => format!("{}={value}", state.name),
            })
            .collect::<Vec<_>>()
            .join(","),
        Err(error) => error.name().to_owned(),
    }
}

/// What two planks stacked in the grid's left column make, and a grid of nine cells but one, as
/// `crafted {id} {count}|none short {id|none}`.
pub(crate) fn craft(ctx: &Callback) -> String {
    let planks = || {
        Some(ItemKey {
            id: "minecraft:oak_planks".to_owned(),
            metadata: 0,
        })
    };
    let mut grid = vec![None; 9];
    grid[0] = planks();
    grid[3] = planks();
    let made = |grid: &[Option<ItemKey>]| {
        ctx.crafting_result(grid)
            .map_or_else(|| "none".to_owned(), |c| format!("{} {}", c.id, c.count))
    };
    format!("crafted {} short {}", made(&grid), made(&grid[..8]))
}

/// Schedules an update of `p` two ticks on, then tries zero ticks and a block it does not own
/// (the one below), as `schedule {outcome} zero {outcome} foreign {outcome}`.
pub(crate) fn schedule(ctx: &Callback, p: BlockPos) -> String {
    let below = BlockPos { y: p.y - 1, ..p };
    format!(
        "schedule {} zero {} foreign {}",
        refusal(ctx.schedule_update(p, 2)),
        refusal(ctx.schedule_update(p, 0)),
        refusal(ctx.schedule_update(below, 2)),
    )
}

/// Moves one item of the container at `pos` from its first slot to its second, and tells
/// `container {slots} moved {id} {first} {second}`, the counts left there, or why it could not:
/// `container {error}` or `container {slots} first empty`.
pub(crate) fn move_in_container(ctx: &Callback, pos: BlockPos) -> String {
    let slots = match ctx.container(pos) {
        Ok(slots) => slots,
        Err(error) => return format!("container {}", error.name()),
    };
    let Some(Some(first)) = slots.first() else {
        return format!("container {} first empty", slots.len());
    };
    let second = slots
        .get(1)
        .cloned()
        .flatten()
        .map_or(0, |stack| stack.count);
    let moved = |count: u8| NewStack {
        id: first.id.clone(),
        metadata: first.metadata,
        count,
        data: first.data.clone(),
    };
    let left = (first.count > 1).then(|| moved(first.count - 1));
    let take = refusal(ctx.set_container_slot(pos, 0, left.as_ref()));
    let put = refusal(ctx.set_container_slot(pos, 1, Some(&moved(second + 1))));
    if take != "ok" || put != "ok" {
        return format!("container take {take} put {put}");
    }
    match ctx.container(pos) {
        Ok(now) => format!(
            "container {} moved {} {} {}",
            now.len(),
            first.id,
            now[0].as_ref().map_or(0, |stack| stack.count),
            now[1].as_ref().map_or(0, |stack| stack.count)
        ),
        Err(error) => format!("container {}", error.name()),
    }
}

/// Tells the redstone power the block at `p` receives, then has it emit 15, as
/// `power {received|error} emit {outcome}`.
pub(crate) fn redstone(ctx: &Callback, p: BlockPos) -> String {
    let received = ctx
        .redstone_power(p)
        .map_or_else(|error| error.name().to_owned(), |power| power.to_string());
    format!(
        "power {received} emit {}",
        refusal(ctx.set_redstone_power(p, 15))
    )
}

/// Tells the actor's game mode, as `game mode {mode|error}`.
pub(crate) fn game_mode(ctx: &Callback) -> String {
    let mode = ctx.game_mode().map_or_else(
        |error| error.name(),
        |mode| match mode {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        },
    );
    format!("game mode {mode}")
}

/// A world error's name, or `ok` for any success.
fn refusal<T>(result: Result<T, WorldError>) -> &'static str {
    result.map_or_else(|error| error.name(), |_| "ok")
}
