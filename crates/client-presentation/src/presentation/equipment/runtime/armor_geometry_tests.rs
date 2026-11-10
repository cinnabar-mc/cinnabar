//! Humanoid armor geometry resolves to the native player armor models.
use crate::presentation::equipment::runtime::canonical_armor_geometry_identifier;

#[test]
fn canonical_humanoid_armor_geometry_aliases_to_native_player_armor() {
    assert_eq!(
        canonical_armor_geometry_identifier("geometry.humanoid.armor.helmet"),
        "geometry.player.armor.helmet"
    );
    assert_eq!(
        canonical_armor_geometry_identifier("geometry.humanoid.armor.chestplate"),
        "geometry.player.armor.chestplate"
    );
    assert_eq!(
        canonical_armor_geometry_identifier("geometry.humanoid.armor.leggings"),
        "geometry.player.armor.leggings"
    );
    assert_eq!(
        canonical_armor_geometry_identifier("geometry.humanoid.armor.boots"),
        "geometry.player.armor.boots"
    );
    assert_eq!(
        canonical_armor_geometry_identifier("geometry.custom.model"),
        "geometry.custom.model"
    );
}
