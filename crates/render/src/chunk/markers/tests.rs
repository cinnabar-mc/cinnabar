use super::*;
use assets::{RuntimeAssets, TerrainMarker};
use meshing::MeshLightSample;

fn key(x: i32, y: i32, z: i32) -> SubChunkKey {
    SubChunkKey {
        dimension: 0,
        x,
        y,
        z,
    }
}

fn marker(local: [u8; 3], class: TerrainMarker) -> PackedTerrainMarker {
    PackedTerrainMarker::new(local, class, 0, MeshLightSample::try_new(13, 4).unwrap())
}

fn instances(
    sections: &TerrainMarkerSections,
    layers: u8,
) -> (Vec<MarkerInstance>, Vec<MarkerInstance>) {
    let textures = ChunkTextureAssets::new(Arc::new(RuntimeAssets::diagnostic()));
    let (mut sprites, mut boxes) = (Vec::new(), Vec::new());
    marker_instances(sections, layers, &textures, &mut sprites, &mut boxes);
    (sprites, boxes)
}

#[test]
fn only_shown_layers_become_instances_at_their_world_cells() {
    let mut sections = TerrainMarkerSections::default();
    sections.set(
        key(1, -1, 2),
        &Arc::from([
            marker([3, 4, 5], TerrainMarker::Light),
            marker([0, 0, 0], TerrainMarker::Barrier),
            marker([15, 15, 15], TerrainMarker::StructureVoid),
        ]),
    );

    assert_eq!(instances(&sections, 0), (Vec::new(), Vec::new()));

    let (sprites, boxes) = instances(&sections, TerrainMarker::Light.layer());
    assert!(boxes.is_empty());
    assert_eq!(sprites.len(), 1);
    assert_eq!(sprites[0].block, [19, -12, 37]);
    assert_eq!((sprites[0].packed >> LIGHT_SHIFT) & 255, 13 | (4 << 4));
    assert_eq!(sprites[0].packed & BOX_FLAG, 0);

    let (sprites, boxes) = instances(&sections, TerrainMarker::StructureVoid.layer());
    assert!(sprites.is_empty());
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].block, [31, -1, 47]);
    assert_eq!(boxes[0].packed & BOX_FLAG, BOX_FLAG);
}

#[test]
fn section_changes_advance_the_revision_only_when_cells_change() {
    let mut sections = TerrainMarkerSections::default();
    let cells: Arc<[PackedTerrainMarker]> = Arc::from([marker([1, 2, 3], TerrainMarker::Barrier)]);
    sections.set(key(0, 0, 0), &cells);
    let revision = sections.revision;
    sections.set(key(0, 0, 0), &Arc::from(cells.to_vec()));
    assert_eq!(sections.revision, revision);

    sections.set(key(0, 0, 0), &Arc::from([]));
    assert!(sections.revision > revision);
    assert!(instances(&sections, u8::MAX).0.is_empty());

    sections.set(key(0, 0, 0), &cells);
    let revision = sections.revision;
    sections.clear();
    assert!(sections.revision > revision);
    assert!(instances(&sections, u8::MAX).0.is_empty());
}
