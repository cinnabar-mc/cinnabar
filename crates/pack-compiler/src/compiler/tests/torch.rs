use super::super::classification::is_torch;
use super::*;
use assets::ModelQuad;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles the matching registry states with one solid texture per block name.
fn compile_fixture(
    admit: impl Fn(&RegistryRecord) -> bool,
) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            admit(record) || matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let names = records
        .iter()
        .filter_map(|record| record.name.strip_prefix("minecraft:"))
        .filter(|name| *name != "air")
        .collect::<std::collections::BTreeSet<_>>();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        serde_json::Value::Object(
            names
                .iter()
                .map(|name| ((*name).to_owned(), serde_json::json!({"textures": name})))
                .collect(),
        )
        .to_string(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::json!({"texture_data": names
            .iter()
            .map(|name| ((*name).to_owned(), serde_json::json!({"textures": format!("textures/blocks/{name}")})))
            .collect::<serde_json::Map<_, _>>()})
        .to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    for (index, name) in names.iter().enumerate() {
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[200, index as u8, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// Every torch-shaped block compiles to a template lit flat by its own cell, with materials that
/// skip ambient occlusion and face dimming, as the native torch tessellation does.
#[test]
fn torches_compile_flat_lit_undimmed_templates() {
    let (compiled, records) = compile_fixture(is_torch);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let torches = records.iter().filter(|record| is_torch(record)).count();
    assert!(torches > 0);
    let flat = assets::MATERIAL_FLAG_DISABLE_AO | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING;
    for record in records.iter().filter(|record| is_torch(record)) {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        assert_eq!(
            template.flags,
            assets::MODEL_TEMPLATE_FLAG_TORCH,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let start = template.quad_start as usize;
        for quad in &runtime.model_quads()[start..start + template.quad_count as usize] {
            assert_eq!(
                runtime.material(quad.material).flags & flat,
                flat,
                "{} {}",
                record.name,
                record.canonical_state
            );
        }
    }
}

/// Quads of the state whose `state` property values all match.
fn state_quads<'a>(
    compiled: &'a assets::CompiledAssets,
    records: &[RegistryRecord],
    name: &str,
    state: &[(&str, &str)],
) -> &'a [ModelQuad] {
    let record = records
        .iter()
        .find(|record| {
            record.name.as_ref() == name
                && state.iter().all(|(property, value)| {
                    let document: serde_json::Value =
                        serde_json::from_str(&record.canonical_state).unwrap();
                    document[property]["value"].to_string().trim_matches('"') == *value
                })
        })
        .unwrap();
    let visual = compiled.visuals[record.sequential_id as usize];
    let template = compiled.model_templates[visual.model_template as usize];
    &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
}

/// Whether a torch quad after the six plate faces is a cap: level and above the plate. Diode
/// torch quads carry no face, so only their geometry tells them apart.
fn is_torch_cap(quad: &ModelQuad) -> bool {
    let y = quad.positions[0][1];
    y > 32 && quad.positions.iter().all(|position| position[1] == y)
}

/// Centres of each torch's top cap above the plate, as `[x, y, z]` in 1/256 block.
fn torch_caps(quads: &[ModelQuad]) -> Vec<[i16; 3]> {
    quads[6..]
        .iter()
        .filter(|quad| is_torch_cap(quad))
        .map(|quad| {
            let sum = |axis: usize| quad.positions.iter().map(|p| p[axis]).sum::<i16>() / 4;
            [sum(0), sum(1), sum(2)]
        })
        .collect()
}

fn is_diode_or_redstone_torch(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:powered_repeater"
            | "minecraft:unpowered_repeater"
            | "minecraft:powered_comparator"
            | "minecraft:unpowered_comparator"
            | "minecraft:redstone_torch"
            | "minecraft:unlit_redstone_torch"
    )
}

#[test]
fn redstone_torches_widen_their_sides_unlike_ordinary_torches() {
    let (compiled, records) =
        compile_fixture(|record| is_torch(record) || is_diode_or_redstone_torch(record));
    let side_width = |name: &str| {
        let quads = state_quads(
            &compiled,
            &records,
            name,
            &[("torch_facing_direction", "top")],
        );
        let side = quads.iter().find(|quad| quad.flags == 3).unwrap();
        let zs = side.positions.map(|p| p[2]);
        zs.iter().max().unwrap() - zs.iter().min().unwrap()
    };
    assert_eq!(side_width("minecraft:redstone_torch"), 64);
    assert_eq!(side_width("minecraft:unlit_redstone_torch"), 64);
    assert_eq!(side_width("minecraft:torch"), 32);
}

#[test]
fn repeater_torches_follow_direction_and_delay() {
    let (compiled, records) = compile_fixture(is_diode_or_redstone_torch);
    let caps = |direction: &str, delay: &str| {
        torch_caps(state_quads(
            &compiled,
            &records,
            "minecraft:unpowered_repeater",
            &[
                ("minecraft:cardinal_direction", direction),
                ("repeater_delay", delay),
            ],
        ))
    };
    // A "south" repeater outputs north: the fixed torch leads and the delay torch slides back.
    assert_eq!(caps("south", "0"), [[128, 110, 112], [128, 110, 48]]);
    assert_eq!(caps("south", "3"), [[128, 110, 208], [128, 110, 48]]);
    assert_eq!(caps("west", "3"), [[48, 110, 128], [208, 110, 128]]);
    assert_eq!(caps("north", "3"), [[128, 110, 48], [128, 110, 208]]);
    assert_eq!(caps("east", "0"), [[112, 110, 128], [48, 110, 128]]);
}

#[test]
fn comparator_torches_follow_direction_and_mode() {
    let (compiled, records) = compile_fixture(is_diode_or_redstone_torch);
    let quads = |name: &str, direction: &str, subtract: &str| {
        state_quads(
            &compiled,
            &records,
            name,
            &[
                ("minecraft:cardinal_direction", direction),
                ("output_subtract_bit", subtract),
            ],
        )
    };
    let unpowered = "minecraft:unpowered_comparator";
    assert_eq!(
        torch_caps(quads(unpowered, "south", "0")),
        [[80, 110, 192], [176, 110, 192], [128, 62, 48]]
    );
    assert_eq!(
        torch_caps(quads(unpowered, "south", "1")),
        [[80, 110, 192], [176, 110, 192], [128, 78, 48]]
    );
    assert_eq!(
        torch_caps(quads(unpowered, "west", "0")),
        [[64, 110, 176], [64, 110, 80], [208, 62, 128]]
    );

    let material = |quads: &[ModelQuad], index: usize| {
        quads[6..]
            .iter()
            .filter(|quad| is_torch_cap(quad))
            .nth(index)
            .unwrap()
            .material
    };
    let lit = state_quads(
        &compiled,
        &records,
        "minecraft:redstone_torch",
        &[("torch_facing_direction", "top")],
    )[0]
    .material;
    let unlit = state_quads(
        &compiled,
        &records,
        "minecraft:unlit_redstone_torch",
        &[("torch_facing_direction", "top")],
    )[0]
    .material;
    let powered = quads("minecraft:powered_comparator", "south", "0");
    assert_eq!(material(powered, 0), lit);
    assert_eq!(material(powered, 2), unlit);
    let subtracting = quads(unpowered, "south", "1");
    assert_eq!(material(subtracting, 0), unlit);
    assert_eq!(material(subtracting, 2), lit);
}

/// Only a plate samples its neighbours' light; a quad with no face takes the diode's own cell.
#[test]
fn diode_torches_carry_no_face_so_they_light_from_their_own_cell() {
    let (compiled, records) = compile_fixture(is_diode_or_redstone_torch);
    let quads = state_quads(
        &compiled,
        &records,
        "minecraft:unpowered_repeater",
        &[
            ("minecraft:cardinal_direction", "south"),
            ("repeater_delay", "0"),
        ],
    );
    assert!(quads.len() > 6);
    let face = |quad: &ModelQuad| quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK;
    assert!(quads[..6].iter().all(|quad| face(quad) != 0));
    assert!(quads[6..].iter().all(|quad| face(quad) == 0));
}

#[test]
fn diode_plates_are_two_pixels_tall_and_turn_their_top_art() {
    let (compiled, records) = compile_fixture(is_diode_or_redstone_torch);
    let top_uv_at_north_west = |direction: &str| {
        let quads = state_quads(
            &compiled,
            &records,
            "minecraft:unpowered_repeater",
            &[
                ("minecraft:cardinal_direction", direction),
                ("repeater_delay", "0"),
            ],
        );
        assert!(
            quads[..6]
                .iter()
                .flat_map(|q| q.positions)
                .all(|p| p[1] <= 32)
        );
        let top = quads[..6].iter().find(|quad| quad.flags == 2).unwrap();
        let corner = top
            .positions
            .iter()
            .position(|position| position[0] == 0 && position[2] == 0)
            .unwrap();
        top.uvs[corner]
    };
    assert_eq!(top_uv_at_north_west("south"), [0, 0]);
    assert_eq!(top_uv_at_north_west("west"), [0, 4096]);
    assert_eq!(top_uv_at_north_west("north"), [4096, 4096]);
    assert_eq!(top_uv_at_north_west("east"), [4096, 0]);
}
