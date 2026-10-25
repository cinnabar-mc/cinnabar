use super::*;

/// Reloaded model chains reserve fresh output credit while older meshes retain their credit.
#[test]
fn resource_reload_refreshes_output_bounds_without_releasing_retained_meshes() {
    let model_id = u32::try_from(light_test_assets().visual_count()).unwrap();
    let source = uniform_sub_chunk(model_id);
    let biome = PackedBiomeRecord::fallback();
    let completion_bytes = std::mem::size_of::<MeshCompletion>() as u64;
    let mut stream = lit_stream_with_assets(1, model_chain_assets(false));
    let old_bound = ::meshing::MeshOutputBounds::new(stream.runtime_assets()).for_sub_chunk(
        &source,
        stream.runtime_assets(),
        stream.network_id_mode(),
    ) + completion_bytes;
    let mut old_permit = stream
        .mesh_memory
        .try_admit(
            &source,
            stream.runtime_assets(),
            stream.network_id_mode(),
            0,
        )
        .unwrap();
    let old_mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    old_permit.reconcile(&old_mesh, &biome);
    let old_bytes = ::meshing::mesh_output_byte_len(&old_mesh, &biome) + completion_bytes;
    let retained = Arc::clone(&stream.mesh_memory.retained);
    assert_eq!(retained.load(Ordering::Acquire), old_bytes);

    stream.reload_resource_assets(Arc::new(model_chain_assets(true)));

    assert!(Arc::ptr_eq(&retained, &stream.mesh_memory.retained));
    assert_eq!(retained.load(Ordering::Acquire), old_bytes);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    let actual = ::meshing::mesh_output_byte_len(&mesh, &biome) + completion_bytes;
    assert!(
        actual > old_bound,
        "the replacement must exceed the old bound"
    );
    let required = ::meshing::MeshOutputBounds::new(stream.runtime_assets()).for_sub_chunk(
        &source,
        stream.runtime_assets(),
        stream.network_id_mode(),
    ) + completion_bytes;
    let mut permit = stream
        .mesh_memory
        .try_admit(
            &source,
            stream.runtime_assets(),
            stream.network_id_mode(),
            0,
        )
        .unwrap();
    assert_eq!(
        retained.load(Ordering::Acquire),
        old_bytes + required,
        "admission must reserve against the replacement assets"
    );
    permit.reconcile(&mesh, &biome);
    assert_eq!(retained.load(Ordering::Acquire), old_bytes + actual);
    drop(old_permit);
    assert_eq!(retained.load(Ordering::Acquire), actual);
    drop(permit);
    assert_eq!(retained.load(Ordering::Acquire), 0);
}

/// Builds a two-quad root that optionally continues through an existing two-part chain.
fn model_chain_assets(extend_root: bool) -> RuntimeAssets {
    let base = light_test_assets();
    let model_id = u32::try_from(base.visual_count()).unwrap();
    let side = assets::MODEL_POSITION_SCALE;
    let uv = assets::MODEL_UV_SCALE;
    let mut quad_start = 0;
    let model_templates = [
        (
            2,
            if extend_root {
                assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
            } else {
                0
            },
        ),
        (
            assets::MAX_MODEL_TEMPLATE_QUADS as u32,
            assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
        ),
        (assets::MAX_MODEL_TEMPLATE_QUADS as u32, 0),
    ]
    .into_iter()
    .map(|(quad_count, flags)| {
        let template = assets::ModelTemplate {
            quad_start,
            quad_count,
            flags,
        };
        quad_start += quad_count;
        template
    })
    .collect();
    let overlay = assets::BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            support: assets::VisualSupport::Exact,
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![LightProperties::new(0, 0).unwrap()],
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        }],
        model_templates,
        model_quads: vec![
            assets::ModelQuad {
                positions: [[0, 0, 0], [0, side, 0], [side, side, 0], [side, 0, 0]],
                uvs: [[0, 0], [0, uv], [uv, uv], [uv, 0]],
                material: 0,
                flags: 0,
            };
            quad_start as usize
        ],
        ..assets::BlockOverlay::default()
    };
    base.with_block_overlay(model_id, &overlay).unwrap()
}
