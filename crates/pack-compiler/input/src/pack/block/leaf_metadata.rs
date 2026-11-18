//! Pack-authored cube UV rotation and world-leaf ambient occlusion metadata.

use super::{BlockTextureMap, Value};
use assets::{
    AssetError, BlockFace, MATERIAL_FLAG_ISOTROPIC, RegistryRecord,
    legacy_resource_pack_block_alias,
};

impl BlockTextureMap {
    pub fn isotropic_face_flags(&self, record: &RegistryRecord) -> [u32; BlockFace::ALL.len()] {
        let name = record
            .name
            .strip_prefix("minecraft:")
            .unwrap_or(&record.name);
        let entry = self
            .entries
            .get(name)
            .or_else(|| self.entries.get(legacy_resource_pack_block_alias(name)?));
        let value = entry.and_then(|entry| entry.extra.get("isotropic"));
        BlockFace::ALL.map(|face| {
            if is_isotropic(value, face) {
                MATERIAL_FLAG_ISOTROPIC
            } else {
                0
            }
        })
    }

    /// World leaf face flags and the f32 bits of an authored `ambient_occlusion_exponent`.
    /// Bedrock stores the authored float unchanged, so any finite number is kept; a missing
    /// exponent is `None` and keeps the material's own.
    pub fn leaf_world_metadata(
        &self,
        record: &RegistryRecord,
    ) -> Result<([u32; BlockFace::ALL.len()], Option<u32>), AssetError> {
        let name = record
            .name
            .strip_prefix("minecraft:")
            .unwrap_or(&record.name);
        let entry = self
            .entries
            .get(name)
            .or_else(|| self.entries.get(legacy_resource_pack_block_alias(name)?));
        let Some(entry) = entry else {
            return Ok(([0; BlockFace::ALL.len()], None));
        };
        let exponent = match entry.extra.get("ambient_occlusion_exponent") {
            None | Some(Value::Null) => None,
            Some(value) => Some(exponent_bits(value).ok_or_else(|| {
                AssetError::InvalidCompiledAssets {
                    detail: format!(
                        "world leaf ambient_occlusion_exponent for {} is not a finite number",
                        record.name
                    )
                    .into(),
                }
            })?),
        };
        Ok((self.isotropic_face_flags(record), exponent))
    }
}

/// The f32 bits of a finite authored exponent, unrounded.
fn exponent_bits(value: &Value) -> Option<u32> {
    let value = value.as_f64()? as f32;
    value.is_finite().then_some(value.to_bits())
}

fn is_isotropic(value: Option<&Value>, face: BlockFace) -> bool {
    // Vanilla scalar bool selects all/none. A non-null side
    // chooses the up/down/side map, otherwise the six explicit face names.
    // Unknown and non-boolean entries do not set bits; caller ignores the
    // helper's failure return, so preserve its admitted mask on odd objects.
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Object(object)) => {
            let key = if face.is_horizontal()
                && object.get("side").is_some_and(|value| !value.is_null())
            {
                "side"
            } else {
                match face {
                    BlockFace::West => "west",
                    BlockFace::East => "east",
                    BlockFace::Down => "down",
                    BlockFace::Up => "up",
                    BlockFace::North => "north",
                    BlockFace::South => "south",
                }
            };
            object.get(key).and_then(Value::as_bool).unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_isotropic_mask_handles_scalar_side_and_odd_objects() {
        for (value, expected) in [
            (serde_json::json!(true), [true; 6]),
            (serde_json::json!(false), [false; 6]),
            (
                serde_json::json!({"up": true, "down": true}),
                [false, false, true, true, false, false],
            ),
            (
                serde_json::json!({"side": false, "west": true, "up": true}),
                [false, false, false, true, false, false],
            ),
            (
                serde_json::json!({"side": true, "east": false, "down": false}),
                [true, true, false, false, true, true],
            ),
            (
                serde_json::json!({"side": null, "north": true, "west": 1, "all": true}),
                [false, false, false, false, true, false],
            ),
            (
                serde_json::json!({"side": 1, "east": true, "up": true}),
                [false, false, false, true, false, false],
            ),
        ] {
            assert_eq!(
                BlockFace::ALL.map(|face| is_isotropic(Some(&value), face)),
                expected
            );
        }
    }

    #[test]
    fn exponents_keep_every_finite_authored_float() {
        for value in [0.0_f32, 0.01, 0.333, 2.56, 9.99, 10.0, 37.5, -0.8] {
            assert_eq!(
                exponent_bits(&serde_json::json!(value)),
                Some(value.to_bits()),
                "{value}"
            );
        }
        assert_eq!(exponent_bits(&serde_json::json!("0.8")), None);
    }
}
