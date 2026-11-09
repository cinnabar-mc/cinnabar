use assets::facts::chorus;

use super::*;

/// Picks the plant's template from what it connects to on each side and from where it sits.
pub(super) fn select_template<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    facts: &PaletteFacts<'a>,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    coordinate: [usize; 3],
    entry: ResolvedPaletteEntry,
) -> u32 {
    let neighbour =
        |face| adjacent_palette_entry(context, facts, neighbour_facts, coordinate, face);
    let connects = |face| {
        model_template_flags(context.visuals, neighbour(face))
            & (assets::MODEL_TEMPLATE_FLAG_CHORUS | assets::MODEL_TEMPLATE_FLAG_CHORUS_PLANT)
            != 0
    };
    let sides = [
        (Face::NegativeZ, chorus::NORTH),
        (Face::PositiveX, chorus::EAST),
        (Face::PositiveZ, chorus::SOUTH),
        (Face::NegativeX, chorus::WEST),
        (Face::PositiveY, chorus::UP),
    ];
    let mut mask = sides
        .into_iter()
        .filter(|&(face, _)| connects(face))
        .fold(0, |mask, (_, bit)| mask | bit);
    if connects(Face::NegativeY)
        || neighbour(Face::NegativeY)
            .flags
            .contains(BlockFlags::CHORUS_ROOT)
    {
        mask |= chorus::DOWN;
    }
    let origin = context.neighbourhood.block_origin();
    let sum = (0..3).fold(0_i32, |sum, axis| {
        sum.wrapping_add(origin[axis].wrapping_add(coordinate[axis] as i32))
    });
    entry.model_template + chorus::template_offset(mask, chorus::position_class(sum))
}
