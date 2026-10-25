//! Compiles emulated geometry programs with a shared resource and varying interface.

use super::*;
use crate::geometry::{GeometryEmulation, lower};
use crate::programs::ProgramGroup;

/// Compiles all stages and the generated vertex reader without linking vertex outputs to fragments.
pub(super) fn translate(
    cx: &Context<'_>,
    sources: &[(usize, Preprocessed)],
    out: &mut ProgramTranslation,
) {
    let fail = |out: &mut ProgramTranslation, message| {
        out.diagnostics
            .push(Diagnostic::new(Phase::Transform, message))
    };
    if !matches!(
        out.program.group,
        ProgramGroup::Gbuffers | ProgramGroup::Shadow
    ) {
        fail(
            out,
            "geometry emulation is only supported for world and shadow programs".into(),
        );
        return;
    }
    let Some((geometry_index, pp)) = sources
        .iter()
        .find(|(i, _)| out.stages[*i].stage == StageKind::Geometry)
    else {
        return;
    };
    let mut lowered = match lower(pp) {
        Ok(lowered) => lowered,
        Err(message) => {
            fail(out, message);
            return;
        }
    };
    let mut iface = ProgramInterface::default();
    for input in &lowered.inputs {
        iface.geometry_input(&input.name, input.location);
    }
    let mut drafts = vec![];
    for (i, source) in sources {
        let stage = out.stages[*i].stage;
        if !matches!(
            stage,
            StageKind::Vertex | StageKind::Fragment | StageKind::Geometry
        ) {
            continue;
        }
        iface.set_geometry_output_interface(stage != StageKind::Vertex);
        let names = ResourceNames {
            image_formats: cx.image_formats,
            sampler_classes: cx.sampler_classes,
            normalize_legacy: normalize_legacy(stage, source),
            geometry: true,
            vertex_clip: false,
        };
        let (source, stage) = if stage == StageKind::Geometry {
            (&lowered.compute, StageKind::Compute)
        } else {
            (source, stage)
        };
        match transform(source, stage, &mut iface, &names) {
            Ok(draft) => drafts.push((*i, draft, source.map.clone())),
            Err(error) => {
                out.stages[*i].result = Err(vec![
                    Diagnostic::new(Phase::Transform, error.message)
                        .at(source.map.locate(error.line)),
                ]);
                return;
            }
        }
    }
    let names = ResourceNames {
        image_formats: cx.image_formats,
        sampler_classes: cx.sampler_classes,
        normalize_legacy: false,
        geometry: true,
        vertex_clip: true,
    };
    iface.set_geometry_output_interface(true);
    let raster = match transform(&lowered.vertex, StageKind::Vertex, &mut iface, &names) {
        Ok(draft) => draft,
        Err(error) => {
            fail(out, error.message);
            return;
        }
    };
    let ubo = match merge_uniforms(drafts.iter().map(|(_, draft, _)| draft)) {
        Ok(ubo) => ubo,
        Err(message) => {
            fail(out, message);
            return;
        }
    };
    let (_, geometry_draft, map) = drafts.iter().find(|(i, ..)| *i == *geometry_index).unwrap();
    let (glsl, origins) = assemble(geometry_draft, &ubo, "");
    let mut compiled = match compile(
        StageKind::Compute,
        &OutputMap {
            glsl: &glsl,
            origins: &origins,
            lines: map,
        },
    ) {
        Ok(compiled) => compiled,
        Err(error) => {
            out.stages[*geometry_index].result = Err(error.diagnostics);
            return;
        }
    };
    lowered.inputs.retain(|field| {
        compiled
            .module
            .global_variables
            .iter()
            .any(|(global, variable)| {
                variable.name.as_deref() == Some(&field.name)
                    && compiled.info.get_entry_point(0)[global]
                        .contains(naga::valid::GlobalUse::READ)
            })
    });
    iface.retain_geometry_inputs(|name| lowered.inputs.iter().any(|field| field.name == name));
    compiled.stage = StageKind::Geometry;
    out.stages[*geometry_index].result = Ok(compiled);
    let locations = match iface.location_defines() {
        Ok(locations) => locations,
        Err(message) => {
            fail(out, message);
            return;
        }
    };
    for (i, draft, map) in &drafts {
        if *i == *geometry_index {
            continue;
        }
        let (glsl, origins) = assemble(draft, &ubo, &locations);
        let stage = out.stages[*i].stage;
        out.stages[*i].result = compile(
            stage,
            &OutputMap {
                glsl: &glsl,
                origins: &origins,
                lines: map,
            },
        )
        .map(|mut c| {
            c.stage = out.stages[*i].stage;
            c
        })
        .map_err(|failure| failure.diagnostics);
    }
    if out.stages.iter().any(|stage| stage.result.is_err()) {
        return;
    }
    let (glsl, origins) = assemble(&raster, &ubo, &locations);
    let mut vertex = match compile(
        StageKind::Vertex,
        &OutputMap {
            glsl: &glsl,
            origins: &origins,
            lines: &lowered.vertex.map,
        },
    ) {
        Ok(vertex) => vertex,
        Err(error) => {
            out.diagnostics.extend(error.diagnostics);
            return;
        }
    };
    let Some(fragment) = out
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Fragment)
        .and_then(|s| s.result.as_ref().ok())
    else {
        fail(out, "geometry program needs a fragment stage".into());
        return;
    };
    if let Err(message) = crate::reflect_link::supply_vertex_outputs(&mut vertex, fragment)
        .and_then(|()| crate::reflect_link::validate(&[&vertex, fragment]))
    {
        fail(out, message);
        return;
    }
    let inputs = drafts
        .iter()
        .map(|(i, draft, _)| {
            (
                out.stages[*i].result.as_ref().unwrap(),
                StageNames {
                    vertex_inputs: &draft.vertex_inputs,
                    frag_outputs: &draft.frag_outputs,
                    resources: &draft.resource_names,
                },
            )
        })
        .collect::<Vec<_>>();
    match reflect(out.program.kind, &inputs, &out.draw_buffers) {
        Ok(mut reflection) => {
            if !reflection.storage_buffers.is_empty() {
                fail(
                    out,
                    "geometry emulation cannot share the pack's storage-buffer group".into(),
                );
                return;
            }
            if let Some(block) = &mut reflection.uniforms {
                block.stages.compute |= block.stages.vertex;
            }
            for binding in &mut reflection.uniform_buffers {
                binding.stages.compute |= binding.stages.vertex;
            }
            for binding in &mut reflection.textures {
                binding.stages.compute |= binding.stages.vertex;
            }
            for binding in &mut reflection.samplers {
                binding.stages.compute |= binding.stages.vertex;
            }
            for binding in &mut reflection.storage_images {
                binding.stages.compute |= binding.stages.vertex;
            }
            let contract = GeometryEmulation {
                max_vertices: lowered.max_vertices,
                input_words: lowered.input_words,
                output_words: lowered.output_words,
                inputs: lowered.inputs,
                outputs: lowered.outputs,
                vertex,
            };
            let original = out
                .stages
                .iter()
                .find(|s| s.stage == StageKind::Vertex)
                .and_then(|s| s.result.as_ref().ok());
            let Some(original) = original else {
                fail(out, "geometry program needs a vertex stage".into());
                return;
            };
            if let Err(errors) = crate::geometry::geometry_vertex_compute(original, &contract, &[])
            {
                out.diagnostics.extend(errors);
                return;
            }
            out.reflection = Some(reflection);
            out.geometry = Some(contract);
        }
        Err(message) => fail(out, message),
    }
}
