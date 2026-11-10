use super::*;

#[test]
fn registration_rejects_unrepresentable_collision_boxes() {
    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        collision: p::EntityBox,
        valid: bool,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../../../../../tools/localserver/experience/testdata/entity_collision_cases.json"
    ))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let manifest = Manifest {
        id: "probe".into(),
        version: String::new(),
        api: String::new(),
        data_schema: 0,
        files: Default::default(),
    };
    for case in cases {
        let definition = w::EntityDefinition {
            id: w::EntityTypeId {
                value: "probe:ship".into(),
            },
            collision: w::EntityBox {
                min: case.collision.min.into(),
                max: case.collision.max.into(),
            },
            appearance: w::EntityAppearance::Vanilla(w::EntityTypeId {
                value: "minecraft:boat".into(),
            }),
            seats: Vec::new(),
            properties: Vec::new(),
        };
        assert_eq!(
            validate(dir.path(), &manifest, vec![definition]).is_ok(),
            case.valid,
            "{}",
            case.name
        );
    }
}
