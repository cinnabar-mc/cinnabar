//! `minecraft:seed` planting rules that an item registry entry declares.

use std::sync::Arc;

use crate::nbt_tree::Nbt;

/// The planting rule a seed item applies when it is used on a block face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSeed {
    /// Block whose default state the item plants, such as `minecraft:wheat`.
    pub crop_result: Arc<str>,
    /// Block names the clicked block may have; names without a namespace are vanilla blocks.
    /// Empty without the solid-surface option means the native default target, farmland.
    pub plant_at: Arc<[Arc<str>]>,
    /// Whether any block that supports the clicked face also accepts the crop.
    pub plant_at_any_solid_surface: bool,
    /// The only clicked face that plants, as a Bedrock face index (0 down, 1 up, 2-5 sides).
    pub plant_at_face: u8,
}

impl ItemSeed {
    /// Counts owned text for the item registry's admission budget.
    pub(super) fn retained_bytes(&self) -> usize {
        self.plant_at.iter().fold(
            std::mem::size_of::<Self>().saturating_add(self.crop_result.len()),
            |bytes, name| {
                bytes
                    .saturating_add(std::mem::size_of::<Arc<str>>())
                    .saturating_add(name.len())
            },
        )
    }
}

/// Reads a seed declaration. An absent face means the top face, as the native item component
/// defaults. A missing crop or an unknown face name leaves the rule unresolved.
pub(super) fn parse_seed(component: Option<&Nbt>) -> Option<ItemSeed> {
    let component = component.filter(|component| matches!(component, Nbt::Compound(_)))?;
    let crop_result = component
        .field("crop_result")
        .and_then(Nbt::as_str)
        .filter(|name| !name.is_empty() && name.len() <= super::components::MAX_TEXT_BYTES)?;
    let plant_at_any_solid_surface = component
        .field("plant_at_any_solid_surface")
        .and_then(Nbt::number)
        .is_some_and(|value| value != 0.0);
    let plant_at: Vec<Arc<str>> = component
        .list("plant_at")
        .iter()
        .filter_map(Nbt::as_str)
        .filter(|name| !name.is_empty() && name.len() <= super::components::MAX_TEXT_BYTES)
        .map(Arc::from)
        .collect();
    let plant_at_face = match component.field("plant_at_face") {
        None => 1,
        Some(face) => face_index(face.as_str()?)?,
    };
    Some(ItemSeed {
        crop_result: Arc::from(crop_result),
        plant_at: plant_at.into(),
        plant_at_any_solid_surface,
        plant_at_face,
    })
}

/// Maps a facing name in either letter case to its Bedrock face index.
fn face_index(name: &str) -> Option<u8> {
    ["down", "up", "north", "south", "west", "east"]
        .iter()
        .position(|face| face.eq_ignore_ascii_case(name))
        .map(|index| index as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(fields: Vec<(String, Nbt)>) -> Option<ItemSeed> {
        parse_seed(Some(&Nbt::Compound(fields)))
    }

    fn text(name: &str, value: &str) -> (String, Nbt) {
        (name.into(), Nbt::String(value.into()))
    }

    #[test]
    fn retail_wheat_seed_declaration_plants_wheat_on_farmland_tops() {
        let parsed = seed(vec![
            text("crop_result", "minecraft:wheat"),
            (
                "plant_at".into(),
                Nbt::List(vec![Nbt::String("minecraft:farmland".into())]),
            ),
            ("plant_at_any_solid_surface".into(), Nbt::Byte(0)),
            text("plant_at_face", "up"),
        ])
        .unwrap();
        assert_eq!(&*parsed.crop_result, "minecraft:wheat");
        assert_eq!(&*parsed.plant_at, [Arc::from("minecraft:farmland")]);
        assert!(!parsed.plant_at_any_solid_surface);
        assert_eq!(parsed.plant_at_face, 1);
    }

    #[test]
    fn absent_targets_stay_empty_and_the_face_defaults_to_the_top() {
        let parsed = seed(vec![text("crop_result", "minecraft:pitcher_crop")]).unwrap();
        assert!(parsed.plant_at.is_empty());
        assert!(!parsed.plant_at_any_solid_surface);
        assert_eq!(parsed.plant_at_face, 1);
        let solid = seed(vec![
            text("crop_result", "minecraft:cave_vines"),
            ("plant_at_any_solid_surface".into(), Nbt::Byte(1)),
            text("plant_at_face", "DOWN"),
        ])
        .unwrap();
        assert!(solid.plant_at.is_empty());
        assert!(solid.plant_at_any_solid_surface);
        assert_eq!(solid.plant_at_face, 0);
    }

    #[test]
    fn missing_crop_or_unknown_face_leaves_the_rule_unresolved() {
        assert_eq!(seed(vec![text("plant_at_face", "up")]), None);
        assert_eq!(
            seed(vec![
                text("crop_result", "minecraft:wheat"),
                text("plant_at_face", "sideways"),
            ]),
            None
        );
        assert_eq!(parse_seed(Some(&Nbt::Byte(1))), None);
    }
}
