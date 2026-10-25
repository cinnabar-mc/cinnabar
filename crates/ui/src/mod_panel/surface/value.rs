//! Binding values: plain scalars and the item and projected values the host resolves.

use serde::{Deserialize, Serialize};

/// Bound for an item identifier named by a surface value.
pub const MAX_ITEM_IDENTIFIER_BYTES: usize = 128;
/// Largest absolute world coordinate a projected point may name, past the world border.
pub const MAX_PROJECTED_COORDINATE: f64 = 32_000_000.0;
/// Largest absolute offset, in blocks, from a projected actor's position.
pub const MAX_PROJECTED_OFFSET: f64 = 256.0;
/// Suffix of the host-published bool beside each projected binding.
pub const PROJECTED_VISIBLE_SUFFIX: &str = "_visible";

/// One binding value. Items and projections are resolved by the host before layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SurfaceValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Vector(Vec<f64>),
    /// `{"item":{"identifier":..,"metadata":..}}`: item artwork bound as
    /// `#item_renderer_data`. Invalid or unknown items draw nothing.
    Item(ItemValue),
    /// `{"project":{"world":[x,y,z]}}` or `{"project":{"actor":id,"offset":[x,y,z]}}`: a
    /// point the host projects every frame to an `[x, y]` GUI offset from the root's top
    /// left, with a bool `<name>_visible` companion.
    Project(ProjectValue),
}

/// The wire wrapper of an item value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemValue {
    pub item: SurfaceItem,
}

/// An item the host draws through the inventory item renderer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceItem {
    pub identifier: String,
    #[serde(default)]
    pub metadata: u32,
}

/// The wire wrapper of a projected value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectValue {
    pub project: Projection,
}

/// What a projected value follows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Projection {
    World(WorldPoint),
    Actor(ActorPoint),
}

/// A fixed world position in blocks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldPoint {
    pub world: [f64; 3],
}

/// An offset in blocks from an actor's interpolated feet position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorPoint {
    pub actor: u64,
    #[serde(default)]
    pub offset: [f64; 3],
}

impl SurfaceValue {
    /// Rejects unbounded values before a host retains extension bindings.
    pub fn validate(&self) -> Result<(), String> {
        let valid = match self {
            Self::Bool(_) => true,
            Self::Number(number) => bounded(number),
            Self::Text(text) => text.len() <= 256 && !text.chars().any(char::is_control),
            Self::Vector(numbers) => {
                (2..=4).contains(&numbers.len()) && numbers.iter().all(bounded)
            }
            Self::Item(ItemValue { item }) => item.valid(),
            Self::Project(ProjectValue { project }) => project.valid(),
        };
        if valid {
            Ok(())
        } else {
            Err("surface binding value exceeds its bounds".into())
        }
    }

    /// The components a transition eases: one for a number, each element of a 2–4-number
    /// array, and none for other values.
    pub fn numbers(&self) -> Option<&[f64]> {
        match self {
            SurfaceValue::Number(number) => Some(std::slice::from_ref(number)),
            SurfaceValue::Vector(numbers) if (2..=4).contains(&numbers.len()) => Some(numbers),
            _ => None,
        }
    }

    /// The projected point, when this value follows one.
    pub fn projection(&self) -> Option<&Projection> {
        match self {
            Self::Project(ProjectValue { project }) => Some(project),
            _ => None,
        }
    }

    /// The item to draw, when this value names one.
    pub fn item(&self) -> Option<&SurfaceItem> {
        match self {
            Self::Item(ItemValue { item }) => Some(item),
            _ => None,
        }
    }
}

impl SurfaceItem {
    /// A bounded namespaced identifier; unknown identifiers are admitted and draw nothing.
    fn valid(&self) -> bool {
        !self.identifier.is_empty()
            && self.identifier.len() <= MAX_ITEM_IDENTIFIER_BYTES
            && self
                .identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-.:/".contains(&byte))
    }
}

impl Projection {
    /// The followed actor's runtime ID, when this projection follows an actor.
    pub fn actor(&self) -> Option<u64> {
        match self {
            Self::World(_) => None,
            Self::Actor(point) => Some(point.actor),
        }
    }

    /// Finite coordinates within the world and offset bounds.
    fn valid(&self) -> bool {
        match self {
            Self::World(point) => point
                .world
                .iter()
                .all(|axis| axis.is_finite() && axis.abs() <= MAX_PROJECTED_COORDINATE),
            Self::Actor(point) => point
                .offset
                .iter()
                .all(|axis| axis.is_finite() && axis.abs() <= MAX_PROJECTED_OFFSET),
        }
    }
}

/// A finite binding number within the surface value bound.
pub(super) fn bounded(number: &f64) -> bool {
    number.is_finite() && number.abs() <= 1_000_000.0
}

/// The host-published visibility binding of a projected binding `name`.
pub fn projected_visible_name(name: &str) -> String {
    format!("{name}{PROJECTED_VISIBLE_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Parses one wire value.
    fn value(json: serde_json::Value) -> Result<SurfaceValue, serde_json::Error> {
        serde_json::from_value(json)
    }

    #[test]
    fn item_and_projected_values_parse_beside_plain_scalars() {
        assert_eq!(
            value(json!({"item":{"identifier":"minecraft:diamond_helmet","metadata":3}})).unwrap(),
            SurfaceValue::Item(ItemValue {
                item: SurfaceItem {
                    identifier: "minecraft:diamond_helmet".into(),
                    metadata: 3,
                },
            })
        );
        assert_eq!(
            value(json!({"project":{"world":[1,64.5,-3]}}))
                .unwrap()
                .projection(),
            Some(&Projection::World(WorldPoint {
                world: [1., 64.5, -3.]
            }))
        );
        let actor = value(json!({"project":{"actor":42,"offset":[0,2,0]}})).unwrap();
        assert_eq!(actor.projection().and_then(Projection::actor), Some(42));
        assert_eq!(
            value(json!({"project":{"actor":7}})).unwrap().projection(),
            Some(&Projection::Actor(ActorPoint {
                actor: 7,
                offset: [0.; 3]
            }))
        );
        assert!(matches!(
            value(json!(2.5)).unwrap(),
            SurfaceValue::Number(_)
        ));
        assert!(matches!(
            value(json!([1, 2])).unwrap(),
            SurfaceValue::Vector(_)
        ));
        let round_trip = json!({"item":{"identifier":"minecraft:apple","metadata":0}});
        assert_eq!(
            serde_json::to_value(value(round_trip.clone()).unwrap()).unwrap(),
            round_trip
        );
    }

    #[test]
    fn item_and_projected_values_reject_unknown_fields_and_unbounded_data() {
        for invalid in [
            json!({"item":{"identifier":"minecraft:apple","count":2}}),
            json!({"item":{"metadata":1}}),
            json!({"item":{"identifier":"minecraft:apple"},"extra":true}),
            json!({"project":{"world":[0,0]}}),
            json!({"project":{"world":[0,0,0],"actor":1}}),
            json!({"project":{"actor":-1}}),
            json!({"other":{}}),
        ] {
            assert!(value(invalid.clone()).is_err(), "{invalid}");
        }
        for unbounded in [
            json!({"item":{"identifier":""}}),
            json!({"item":{"identifier":"minecraft:bad name"}}),
            json!({"item":{"identifier":"x".repeat(MAX_ITEM_IDENTIFIER_BYTES + 1)}}),
            json!({"project":{"world":[0,MAX_PROJECTED_COORDINATE * 2.,0]}}),
            json!({"project":{"actor":1,"offset":[0,MAX_PROJECTED_OFFSET + 1.,0]}}),
        ] {
            assert!(
                value(unbounded.clone()).unwrap().validate().is_err(),
                "{unbounded}"
            );
        }
        assert!(
            value(json!({"item":{"identifier":"minecraft:potion","metadata":36}}))
                .unwrap()
                .validate()
                .is_ok()
        );
    }
}
