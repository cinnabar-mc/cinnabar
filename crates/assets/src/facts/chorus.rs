//! Shared layout of the chorus plant's neighbour- and position-selected templates.

/// Connection bits, one per direction a plant can reach a neighbour in.
pub const NORTH: u32 = 1 << 0;
pub const EAST: u32 = 1 << 1;
pub const SOUTH: u32 = 1 << 2;
pub const WEST: u32 = 1 << 3;
pub const UP: u32 = 1 << 4;
pub const DOWN: u32 = 1 << 5;
/// Connection masks times the four position classes that vary unconnected stubs.
pub const TEMPLATE_COUNT: u32 = 64 * POSITION_CLASSES;
/// How many stub arrangements a plant's coordinates select between.
pub const POSITION_CLASSES: u32 = 4;

/// Position class of a block: the size of its truncated remainder of the coordinate sum by four.
#[must_use]
pub const fn position_class(coordinate_sum: i32) -> u32 {
    (coordinate_sum % POSITION_CLASSES as i32).unsigned_abs()
}

/// Offset of a connection mask and position class from a material's first plant template.
#[must_use]
pub const fn template_offset(connections: u32, class: u32) -> u32 {
    connections | class << 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_class_truncates_negative_sums_toward_zero() {
        assert_eq!(position_class(5), 1);
        assert_eq!(position_class(-5), 1);
        assert_eq!(position_class(-4), 0);
        assert_eq!(position_class(-7), 3);
    }

    #[test]
    fn every_mask_and_class_has_its_own_template() {
        assert_eq!(template_offset(0, 0), 0);
        assert_eq!(
            template_offset(
                NORTH | EAST | SOUTH | WEST | UP | DOWN,
                POSITION_CLASSES - 1
            ) + 1,
            TEMPLATE_COUNT
        );
    }
}
