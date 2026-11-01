struct CompiledChorusFixture {
    assets: RuntimeAssets,
    air: u32,
    plant: u32,
    flower: u32,
    end_stone: u32,
    stone: u32,
}

fn write_chorus_pack(root: &Path) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create chorus fixture tree");
    fs::write(
        root.join("blocks.json"),
        r#"{
            "chorus_plant":{"textures":"chorus_plant"},
            "chorus_flower":{"textures":"chorus_flower"},
            "end_stone":{"textures":"end_stone"},
            "stone":{"textures":"stone"}
        }"#,
    )
    .expect("write chorus block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "chorus_plant":{"textures":"textures/blocks/chorus_plant"},
            "chorus_flower":{"textures":"textures/blocks/chorus_flower"},
            "end_stone":{"textures":"textures/blocks/end_stone"},
            "stone":{"textures":"textures/blocks/stone"}
        }}"#,
    )
    .expect("write chorus terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write chorus empty flipbooks");
    for (index, name) in ["chorus_plant", "chorus_flower", "end_stone", "stone"]
        .into_iter()
        .enumerate()
    {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                &[30 + index as u8 * 40, 70, 110, 255].repeat(16 * 16),
                16,
                16,
                ExtendedColorType::Rgba8,
            )
            .expect("encode chorus fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write chorus fixture PNG");
    }
}

fn compiled_chorus_fixture() -> &'static CompiledChorusFixture {
    static FIXTURE: OnceLock<CompiledChorusFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!(
            "../../../../assets/data/block-registry-v1001.bin"
        ))
        .expect("decode chorus registry");
        let first = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("{name} record"))
                .clone()
        };
        let selected = [
            first("minecraft:air"),
            first("minecraft:chorus_plant"),
            first("minecraft:chorus_flower"),
            first("minecraft:end_stone"),
            first("minecraft:stone"),
        ];
        let directory = tempfile::tempdir().expect("chorus fixture directory");
        write_chorus_pack(directory.path());
        let compiled = compile_pack(directory.path(), &selected).expect("compile chorus fixture");
        let blob = encode_blob(&compiled).expect("encode chorus fixture");
        CompiledChorusFixture {
            assets: RuntimeAssets::decode(&blob).expect("decode chorus fixture"),
            air: selected[0].sequential_id,
            plant: selected[1].sequential_id,
            flower: selected[2].sequential_id,
            end_stone: selected[3].sequential_id,
            stone: selected[4].sequential_id,
        }
    })
}

/// Template offset the mesher selects for a plant at `[8, 8, 8]` amid the given blocks.
fn plant_template_offset(fixture: &CompiledChorusFixture, placed: &[([u8; 3], u32)]) -> u32 {
    let palette = std::iter::once(fixture.air)
        .chain(std::iter::once(fixture.plant))
        .chain(placed.iter().map(|&(_, block)| block))
        .collect::<Vec<_>>();
    let mut placements = vec![([8, 8, 8], 1_usize)];
    placements.extend(
        placed
            .iter()
            .enumerate()
            .map(|(index, &(position, _))| (position, index + 2)),
    );
    let center = sub_chunk(vec![packed_storage(4, &palette, &placements)]);
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(fixture.air),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &center,
    );
    let base = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.plant)
        .model_template()
        .expect("chorus plant template");
    let cell = 8 | (8 << 4) | (8 << 8);
    mesh.model_refs()
        .iter()
        .map(|model| model.words())
        .find(|words| words[0] == cell)
        .expect("plant model ref")[1]
        - base
}

#[test]
fn chorus_plants_draw_their_art_and_connect_only_to_their_family_and_root() {
    use assets::facts::chorus;
    let fixture = compiled_chorus_fixture();
    let visual = fixture
        .assets
        .resolve(NetworkIdMode::Sequential, fixture.plant);
    assert_eq!(visual.kind(), VisualKind::Model);
    assert_ne!(visual.support(), assets::VisualSupport::Diagnostic);

    // The cell sums to 24, the first of the four position classes.
    let lone = plant_template_offset(fixture, &[]);
    assert_eq!(lone, 0);
    assert_eq!(
        plant_template_offset(fixture, &[([8, 9, 8], fixture.flower)]),
        chorus::UP
    );
    assert_eq!(
        plant_template_offset(fixture, &[([9, 8, 8], fixture.plant)]),
        chorus::EAST
    );
    assert_eq!(
        plant_template_offset(fixture, &[([8, 8, 7], fixture.plant)]),
        chorus::NORTH
    );
    assert_eq!(
        plant_template_offset(fixture, &[([8, 7, 8], fixture.end_stone)]),
        chorus::DOWN
    );
    assert_eq!(
        plant_template_offset(fixture, &[([8, 7, 8], fixture.stone)]),
        0,
        "other ground gets no root"
    );
    assert_eq!(
        plant_template_offset(fixture, &[([9, 8, 8], fixture.stone)]),
        0,
        "other blocks get no arm"
    );
}
