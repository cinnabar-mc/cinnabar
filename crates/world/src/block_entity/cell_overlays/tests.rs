use std::cell::Cell;
use std::sync::Arc;

use super::{CellOverlay, OverlayItem, OverlayLook, ResolvedCellOverlays};
use crate::{BlockEntityNbt, BlockIds, NbtCompound, NbtValue};

/// The block entity the server adapter sends for a `probe:bus` carrying a plate on its north face
/// in slot 2 and a diamond on its top in slot 5, written by its `TestCellOverlaysFixtureIsCurrent`.
const SERVER_FIXTURE: &[u8] =
    include_bytes!("../../../../../tools/localserver/experience/testdata/go/cell_overlays.nbt");

/// Resolves block state compounds by name: `probe:bus` is 7, `probe:plate` 9, anything else air
/// (0). Its registry identity is `generation`, and it counts resolutions.
struct NamedIds {
    generation: u32,
    resolved: Cell<usize>,
}

impl NamedIds {
    fn new(generation: u32) -> Self {
        Self {
            generation,
            resolved: Cell::new(0),
        }
    }
}

impl BlockIds for NamedIds {
    fn inline_cache_context(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        Some(Arc::new(self.generation))
    }

    fn inline_cache_context_matches(&self, context: &(dyn std::any::Any + Send + Sync)) -> bool {
        context.downcast_ref::<u32>() == Some(&self.generation)
    }

    fn air(&self) -> u32 {
        0
    }

    fn resolve(&self, network_id: u32) -> u32 {
        network_id
    }

    fn resolve_persistent(&self, entry: &NbtCompound) -> u32 {
        self.resolved.set(self.resolved.get() + 1);
        match entry.string("name") {
            Some("probe:bus") => 7,
            Some("probe:plate") => 9,
            _ => 0,
        }
    }
}

/// Encodes `root` through the public wire ingestion boundary.
fn decode(root: NbtCompound) -> BlockEntityNbt {
    BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0
}

/// A block state compound of `name` without states.
fn state(name: &str) -> NbtValue {
    let mut state = NbtCompound::default();
    state.insert("name", NbtValue::String(name.into()));
    state.insert("states", NbtValue::Compound(NbtCompound::default()));
    NbtValue::Compound(state)
}

/// An overlay entry in `slot` on `face` drawing `name`'s model.
fn block_entry(slot: i8, face: i8, name: &str) -> NbtValue {
    let mut entry = NbtCompound::default();
    entry.insert("slot", NbtValue::Byte(slot));
    entry.insert("face", NbtValue::Byte(face));
    entry.insert("block", state(name));
    NbtValue::Compound(entry)
}

/// An overlay entry in `slot` on the north face drawing a diamond `size` pixels wide at `x`, `y`.
fn item_entry(slot: i8, size: f32, x: f32, y: f32) -> NbtValue {
    let mut item = NbtCompound::default();
    item.insert("Name", NbtValue::String("minecraft:diamond".into()));
    item.insert("Damage", NbtValue::Short(0));
    let mut entry = NbtCompound::default();
    entry.insert("slot", NbtValue::Byte(slot));
    entry.insert("face", NbtValue::Byte(2));
    entry.insert("item", NbtValue::Compound(item));
    entry.insert("size", NbtValue::Float(size));
    entry.insert("x", NbtValue::Float(x));
    entry.insert("y", NbtValue::Float(y));
    NbtValue::Compound(entry)
}

/// A `CinnabarOverlays` block entity of a `probe:bus` holding `entries`.
fn overlays(entries: Vec<NbtValue>) -> NbtCompound {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("CinnabarOverlays".into()));
    root.insert("host", state("probe:bus"));
    root.insert("overlays", NbtValue::List(entries));
    root
}

/// A plate on the north face of a probe:bus.
fn plate(slot: u8, face: u8) -> CellOverlay {
    CellOverlay {
        slot,
        face,
        look: OverlayLook::Block(9),
    }
}

#[test]
fn the_server_fixture_decodes_to_its_host_and_overlays() {
    let (nbt, consumed) = BlockEntityNbt::decode_prefix(SERVER_FIXTURE).unwrap();
    assert_eq!(consumed, SERVER_FIXTURE.len());
    let resolved = nbt.cell_overlays().unwrap().resolve(&NamedIds::new(1));
    let expected = [
        plate(2, 2),
        CellOverlay {
            slot: 5,
            face: 1,
            look: OverlayLook::Item(OverlayItem {
                name: Arc::from("minecraft:diamond"),
                damage: 0,
                size: 6.4,
                x: 0.0,
                y: 0.8,
            }),
        },
    ];
    assert_eq!(resolved.on(7), Some(&expected[..]));
    assert_eq!(resolved.on(9), None, "a replaced host shows no overlays");
}

#[test]
fn entries_breaking_the_format_are_skipped() {
    let nbt = decode(overlays(vec![
        block_entry(1, 2, "probe:plate"),
        block_entry(1, 3, "probe:plate"),
        block_entry(0, 3, "probe:plate"),
        block_entry(2, 6, "probe:plate"),
        block_entry(3, 1, "probe:unknown"),
        item_entry(4, 16.5, 0.0, 0.0),
        item_entry(4, 4.0, 8.5, 0.0),
        item_entry(4, f32::NAN, 0.0, 0.0),
        item_entry(4, 0.0, 0.0, 0.0),
        block_entry(12, 0, "probe:plate"),
        block_entry(11, 0, "probe:plate"),
    ]));
    let decoded = nbt.cell_overlays().unwrap();
    assert_eq!(decoded.len(), 3, "slots 1, 3 and 11 decode");
    assert_eq!(
        *decoded.resolve(&NamedIds::new(1)),
        ResolvedCellOverlays {
            host: 7,
            overlays: Box::new([plate(1, 2), plate(11, 0)]),
        },
        "an unknown block state draws nothing"
    );

    let mut without_host = overlays_root_without_host();
    without_host.insert(
        "overlays",
        NbtValue::List(vec![block_entry(0, 0, "probe:plate")]),
    );
    assert!(decode(without_host).cell_overlays().is_none());
    let mut chest = overlays(Vec::new());
    chest.insert("id", NbtValue::String("Chest".into()));
    assert!(decode(chest).cell_overlays().is_none());
    let numbers = decode(overlays(vec![NbtValue::Int(5), NbtValue::Int(6)]));
    assert!(
        numbers.cell_overlays().unwrap().is_empty(),
        "entries that are no compounds"
    );
}

/// An overlays block entity without its host compound.
fn overlays_root_without_host() -> NbtCompound {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("CinnabarOverlays".into()));
    root
}

#[test]
fn resolution_is_reused_while_the_registry_stays_the_same() {
    let nbt = decode(overlays(vec![block_entry(0, 0, "probe:plate")]));
    let overlays = nbt.cell_overlays().unwrap();
    let first = NamedIds::new(1);
    let resolved = overlays.resolve(&first);
    assert_eq!(first.resolved.get(), 2, "the host and the plate");
    assert!(Arc::ptr_eq(&resolved, &overlays.resolve(&first)));
    assert_eq!(first.resolved.get(), 2);
    let second = NamedIds::new(2);
    assert!(!Arc::ptr_eq(&resolved, &overlays.resolve(&second)));
    assert_eq!(second.resolved.get(), 2);
}
