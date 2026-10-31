use super::*;

/// Supplies a tiny registry with one opaque cube and an explicit air identity.
fn assets() -> RuntimeAssets {
    use assets::*;
    let base = RuntimeAssets::diagnostic();
    let visual = BlockVisual {
        native_material: native_material::NativeMaterialFacts::Unresolved,
        faces: [0; 6],
        flags: BlockFlags::AIR,
        kind: VisualKind::Invisible,
        contributor_role: ContributorRole::Air,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        support: VisualSupport::Exact,
        destroy: compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    base.with_block_overlay(
        1,
        &BlockOverlay {
            visuals: vec![
                visual,
                BlockVisual {
                    kind: VisualKind::Cube,
                    flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                    contributor_role: ContributorRole::Primary,
                    ..visual
                },
            ],
            light_properties: vec![LightProperties::default(), LightProperties::OPAQUE_DARK],
            materials: vec![Material {
                texture: TextureRef::DIAGNOSTIC,
                ..Material::unvaried()
            }],
            ..Default::default()
        },
    )
    .unwrap()
}

/// Builds a solid rectangular grid with distinct solved light channels.
fn grid(size: [u32; 3]) -> BlockGrid {
    BlockGrid {
        revision: 0,
        size,
        palette: vec![BlockState::RuntimeId(2)],
        cells: vec![
            BlockCell {
                palette: 0,
                sky_light: Some(11),
                block_light: Some(4)
            };
            size.into_iter().map(|v| v as usize).product()
        ],
    }
}

#[test]
fn block_grid_admission_checks_shape_palette_and_light() {
    let mut candidate = grid([1; 3]);
    candidate.cells[0].sky_light = Some(16);
    assert_eq!(candidate.admit().unwrap_err().code, ErrorCode::Invalid);
    let mut candidate = grid([1; 3]);
    candidate.cells[0].palette = 1;
    assert_eq!(candidate.admit().unwrap_err().code, ErrorCode::Invalid);
    let mut candidate = grid([1; 3]);
    candidate.cells.clear();
    assert_eq!(candidate.admit().unwrap_err().code, ErrorCode::Invalid);
    let mut candidate = grid([1; 3]);
    candidate.size = [u32::MAX; 3];
    assert_eq!(
        candidate.admit().unwrap_err().code,
        ErrorCode::BudgetExceeded
    );
    let geometry = grid([48, 256, 48]).admit().unwrap();
    assert_eq!(geometry.bounds(), [[0.; 3], [48., 256., 48.]]);
}

#[test]
fn block_grid_uses_identical_terrain_meshes_and_solved_light() {
    let assets = assets();
    let grid = grid([16; 3]);
    let actual = grid
        .mesh(
            &assets,
            NetworkIdMode::Sequential,
            |_, _| None,
            Default::default(),
        )
        .unwrap();
    let section = world::SubChunk::from_runtime_ids(&[2; world::BLOCKS_PER_SUB_CHUNK]);
    let expected = meshing::mesh_sub_chunk_in_neighbourhood_with_lighting(
        &meshing::BlockClassifier::new(1),
        &assets,
        NetworkIdMode::Sequential,
        &world::MeshNeighbourhood::new(&section),
        &|p: [i32; 3]| {
            if p.into_iter().all(|v| (0..16).contains(&v)) {
                meshing::MeshLightSample::try_new(4, 11).unwrap()
            } else {
                meshing::MeshLightSample::FULL_BRIGHT
            }
        },
    );
    assert_eq!(actual.sections.len(), 1);
    assert_eq!(actual.sections[0].mesh, expected);
}

#[test]
fn block_grid_culls_internal_section_boundaries_but_keeps_outer_faces() {
    let assets = assets();
    let actual = grid([32, 16, 16])
        .mesh(
            &assets,
            NetworkIdMode::Sequential,
            |_, _| None,
            Default::default(),
        )
        .unwrap();
    assert_eq!(actual.sections.len(), 2);
    assert_eq!(
        actual
            .sections
            .iter()
            .map(|s| s.mesh.cube_quads().len())
            .sum::<usize>(),
        10
    );
}

#[test]
fn completed_grid_does_no_work_on_idle_status_reads() {
    let geometry = grid([1; 3]).admit().unwrap();
    let request = geometry.blocks().unwrap();
    assert!(request.claim());
    assert!(!request.claim());
    assert!(request.result().is_none());
    request.complete(Ok(BlockMesh {
        sections: vec![],
        triangles: 0,
        bytes: 0,
    }));
    let result = request.result().unwrap().as_ref().unwrap();
    for _ in 0..100 {
        assert!(!request.claim());
        assert!(Arc::ptr_eq(
            result,
            request.result().unwrap().as_ref().unwrap()
        ));
    }
}

#[test]
fn named_states_and_unknown_states_have_explicit_outcomes() {
    let assets = assets();
    let mut grid = grid([1; 3]);
    grid.palette[0] = BlockState::Named {
        name: "test:cube".into(),
        states: "{}".into(),
    };
    assert_eq!(
        grid.mesh(
            &assets,
            NetworkIdMode::Sequential,
            |_, _| None,
            Default::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::Invalid
    );
    assert!(
        grid.mesh(
            &assets,
            NetworkIdMode::Sequential,
            |name, states| (name == "test:cube" && states == "{}").then_some(2),
            Default::default()
        )
        .is_ok()
    );
}

#[test]
fn small_edits_share_unchanged_sections_and_noop_edits_preserve_identity() {
    let assets = Arc::new(assets());
    let mut store = super::super::Store::default();
    let id = store
        .create_geometry(super::super::GeometrySpec::Blocks(grid([48, 16, 16])))
        .unwrap();
    store.commit();
    let old = Arc::clone(&store.snapshot().geometry[&id]);
    let request = old.blocks().unwrap();
    assert!(request.bind_context(&assets, NetworkIdMode::Sequential, Default::default()));
    request.complete(request.mesh(
        &assets,
        NetworkIdMode::Sequential,
        |_, _| None,
        Default::default(),
    ));
    request.complete_render();
    let original = Arc::clone(request.result().unwrap().as_ref().unwrap());
    let original_cell = request.grid.cell([1, 1, 1]).unwrap();
    store.begin();
    store
        .update_blocks(
            id,
            vec![BlockEdit {
                position: [1; 3],
                cell: original_cell,
            }],
            DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    assert!(Arc::ptr_eq(&old, &store.snapshot().geometry[&id]));
    store.begin();
    store
        .update_blocks(
            id,
            vec![BlockEdit {
                position: [1; 3],
                cell: BlockCell {
                    sky_light: Some(3),
                    ..original_cell
                },
            }],
            DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    let changed = store.snapshot().geometry[&id].blocks().unwrap();
    let mesh = changed
        .mesh(
            &assets,
            NetworkIdMode::Sequential,
            |_, _| None,
            Default::default(),
        )
        .unwrap();
    for origin in [[16, 0, 0], [32, 0, 0]] {
        assert!(Arc::ptr_eq(
            original
                .sections
                .iter()
                .find(|section| section.origin == origin)
                .unwrap(),
            mesh.sections
                .iter()
                .find(|section| section.origin == origin)
                .unwrap()
        ));
    }
}

#[test]
fn process_feedback_is_fenced_to_the_grid_generation() {
    let mut store = super::super::Store::default();
    let id = store
        .create_geometry(super::super::GeometrySpec::Blocks(grid([1; 3])))
        .unwrap();
    store.commit();
    let request = store.snapshot().geometry[&id].blocks().unwrap();
    request.accept_status(1, Ok(()));
    assert_eq!(store.geometry_status(id), Ok(false));
    request.accept_status(0, Ok(()));
    assert_eq!(store.geometry_status(id), Ok(true));
    store
        .update_blocks(
            id,
            vec![BlockEdit {
                position: [0; 3],
                cell: BlockCell {
                    palette: 0,
                    sky_light: Some(2),
                    block_light: None,
                },
            }],
            DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    let request = store.snapshot().geometry[&id].blocks().unwrap();
    assert_eq!(request.grid.revision, 1);
    request.accept_status(0, Ok(()));
    assert_eq!(store.geometry_status(id), Ok(false));
    request.accept_status(1, Err(ErrorCode::BudgetExceeded));
    assert_eq!(
        store.geometry_status(id).unwrap_err().code,
        ErrorCode::BudgetExceeded
    );
}

#[test]
fn transferred_grids_require_material_authority_even_without_instances() {
    let mut store = super::super::Store::default();
    store
        .create_geometry(super::super::GeometrySpec::Blocks(grid([1; 3])))
        .unwrap();
    store.commit();
    let snapshot = super::super::transfer::Snapshot::from_resources(store.snapshot());
    assert_eq!(
        snapshot
            .admit(
                None,
                DrawingPermissions {
                    render_material: false,
                    ..DrawingPermissions::ALL
                },
                1,
                0,
                |_| true,
                |id| id
            )
            .unwrap_err()
            .code,
        ErrorCode::Denied
    );
}

#[test]
fn repeated_placements_share_mesh_storage_but_each_consumes_draw_work() {
    let mut store = super::super::Store::default();
    let id = store
        .create_geometry(super::super::GeometrySpec::Blocks(grid([1; 3])))
        .unwrap();
    store.commit();
    store.snapshot().geometry[&id]
        .blocks()
        .unwrap()
        .complete(Ok(BlockMesh {
            sections: vec![],
            triangles: MAX_BLOCK_TRIANGLES,
            bytes: 0,
        }));
    let instance = super::super::tests::drawing(id);
    store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    assert_eq!(
        store
            .create(instance, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    store.commit();
    assert_eq!(store.snapshot().instances.len(), 2);
}

#[test]
fn readiness_waits_for_gpu_admission_and_late_errors_stay_visible() {
    let geometry = grid([1; 3]).admit().unwrap();
    let request = geometry.blocks().unwrap();
    request.complete(Ok(BlockMesh {
        sections: vec![],
        triangles: 0,
        bytes: 0,
    }));
    assert!(request.status().is_none());
    request.complete_render();
    assert_eq!(request.status(), Some(Ok(())));
    request.fail_render(error(ErrorCode::BudgetExceeded, "test arena"));
    assert_eq!(request.status(), Some(Err(ErrorCode::BudgetExceeded)));
}

#[test]
fn retained_partial_edits_keep_the_context_of_reused_sections_before_the_next_prepare() {
    let assets = Arc::new(assets());
    let mut store = super::super::Store::default();
    let id = store
        .create_geometry(super::super::GeometrySpec::Blocks(grid([32, 1, 1])))
        .unwrap();
    store.commit();
    let old = Arc::clone(&store.snapshot().geometry[&id]);
    let request = old.blocks().unwrap();
    let settings = meshing::MeshRenderSettings::default();
    assert!(request.bind_context(&assets, NetworkIdMode::Sequential, settings));
    request.complete(request.mesh(&assets, NetworkIdMode::Sequential, |_, _| None, settings));
    store
        .update_blocks(
            id,
            vec![BlockEdit {
                position: [0; 3],
                cell: BlockCell {
                    block_light: Some(9),
                    ..request.grid.cells[0]
                },
            }],
            DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    let edited = store.snapshot().geometry[&id].blocks().unwrap();
    assert!(!edited.bind_context(
        &assets,
        NetworkIdMode::Sequential,
        meshing::MeshRenderSettings {
            smooth_lighting: !settings.smooth_lighting,
            ..settings
        }
    ));
    assert!(!edited.bind_context(
        &Arc::new(self::assets()),
        NetworkIdMode::Sequential,
        settings
    ));
    assert!(edited.bind_context(&assets, NetworkIdMode::Sequential, settings));
}
