use super::*;
use assets::{MODEL_TEMPLATE_FLAG_SHEET_TILES as TILES, MODEL_TEMPLATE_FLAG_SHELF as SHELF};

#[test]
fn decode_requires_sheet_tile_templates_to_precede_their_whole_sheet_pair() {
    let canonical = compound_blob();
    let templates = read_u64(&canonical, 216) as usize;
    for (first, second, valid) in [
        (SHELF | TILES, SHELF, true),
        (SHELF | TILES, 0, false),
        (SHELF, SHELF | TILES, false),
        (assets::MODEL_TEMPLATE_FLAG_KELP | TILES, 0, false),
    ] {
        let mut blob = canonical.clone();
        write_u32(&mut blob, templates + 8, first);
        write_u32(&mut blob, templates + 12 + 8, second);
        reseal(&mut blob);
        assert_eq!(
            RuntimeAssets::decode(&blob).is_ok(),
            valid,
            "{first:#x} {second:#x}"
        );
    }
}
