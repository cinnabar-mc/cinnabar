use super::*;
use crate::{Aabb, OverlayHit};

/// A cable core: the host's own selection box.
const CORE: ShapeBox = ShapeBox::new([0.375; 3], [0.625; 3]);
/// The overlay state's id: a plate two pixels deep against the north face it is authored on.
const PLATE: u32 = 9;

/// Resolves `probe:bus` to the host id 7 and `probe:plate` to [`PLATE`], anything else to air.
struct ProbeIds;

impl world::BlockIds for ProbeIds {
    fn air(&self) -> u32 {
        0
    }

    fn resolve(&self, network_id: u32) -> u32 {
        network_id
    }

    fn resolve_persistent(&self, entry: &NbtCompound) -> u32 {
        match entry.string("name") {
            Some("probe:bus") => 7,
            Some("probe:plate") => PLATE,
            _ => 0,
        }
    }
}

/// A `probe:bus` core at the origin carrying a plate in each `(slot, face)`.
fn bus(plates: &[(i8, i8)]) -> (ChunkStore, CollisionRegistry) {
    let (mut store, mut registry, id) = fixture(
        CollisionIdSpace::Sequential,
        &shapes(SelectionContext::Static, vec![CORE], vec![]),
    );
    assert_eq!(id, 7);
    registry.register(PLATE, []).unwrap();
    assert!(registry.set_pick_shapes(
        PLATE,
        [box_to_aabb(ShapeBox::new([0.0; 3], [1.0, 1.0, 0.125]))]
    ));
    let named = |name: &str| {
        let mut state = NbtCompound::default();
        state.insert("name", NbtValue::String(name.into()));
        NbtValue::Compound(state)
    };
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("CinnabarOverlays".into()));
    root.insert("host", named("probe:bus"));
    root.insert(
        "overlays",
        NbtValue::List(
            plates
                .iter()
                .map(|&(slot, face)| {
                    let mut entry = NbtCompound::default();
                    entry.insert("slot", NbtValue::Byte(slot));
                    entry.insert("face", NbtValue::Byte(face));
                    entry.insert("block", named("probe:plate"));
                    NbtValue::Compound(entry)
                })
                .collect(),
        ),
    );
    let nbt = BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0;
    store
        .commit_block_entity_update(BlockEntityKey::new(0, 0, 0, 0), nbt)
        .unwrap();
    (store, registry)
}

/// The interaction ray from south of the bus straight north through its centre.
fn from_south(world: &PaletteWorld<'_>) -> crate::BlockHit {
    world
        .block_interaction_ray_current(Vec3::new(0.5, 0.5, 2.5), Vec3::new(0.0, 0.0, -1.0), 4.0)
        .unwrap()
        .unwrap()
}

#[test]
fn the_ray_outlines_a_turned_overlay_and_reports_its_face_and_position() {
    let (store, registry) = bus(&[(3, 3)]);
    let ids = ProbeIds;
    let world =
        PaletteWorld::new(&store, &registry, 0, Default::default()).with_cell_overlays(&ids);
    let hit = from_south(&world);
    assert_eq!((hit.block_pos, hit.face, hit.runtime_id), ([0; 3], 3, 7));
    assert_eq!(hit.hit_local, Vec3::new(0.5, 0.5, 1.0));
    assert_eq!(
        hit.overlay,
        Some(OverlayHit {
            slot: 3,
            outline: Aabb::new(Vec3::new(0.0, 0.0, 0.875), Vec3::new(1.0, 1.0, 1.0)),
        }),
        "the south plate, turned from north, is the outline"
    );

    let plain = PaletteWorld::new(&store, &registry, 0, Default::default());
    let core = from_south(&plain);
    assert_eq!(
        (core.overlay, core.hit_local.z),
        (None, 0.625),
        "no resolver, no overlays"
    );
}

#[test]
fn the_lower_slot_wins_a_tie_and_overlays_leave_with_their_host() {
    let (mut store, mut registry) = bus(&[(2, 3), (5, 3)]);
    let ids = ProbeIds;
    {
        let world =
            PaletteWorld::new(&store, &registry, 0, Default::default()).with_cell_overlays(&ids);
        assert_eq!(from_south(&world).overlay.map(|hit| hit.slot), Some(2));
    }
    registry.register(8, [box_to_aabb(CORE)]).unwrap();
    set_block(&mut store, [0; 3], 8);
    let world =
        PaletteWorld::new(&store, &registry, 0, Default::default()).with_cell_overlays(&ids);
    let hit = from_south(&world);
    assert_eq!(
        (hit.runtime_id, hit.overlay),
        (8, None),
        "another block shows no overlays"
    );
}
