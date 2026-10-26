//! Validate the Vulkan programs after translation, including runtime robustness code.

use std::collections::{BTreeMap, BTreeSet};
use std::{path::Path, process::Command};

#[test]
fn terrain_spirv_validates_with_mobile_runtime_checks() {
    let validator = std::env::var_os("SPIRV_VAL").unwrap_or_else(|| "spirv-val".into());
    let available = Command::new(&validator).arg("--version").output().is_ok();
    if !available {
        eprintln!("missing fixture: spirv-val for native Vulkan shader validation");
    }
    let output = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("cube", include_str!("../chunk.wesl")),
        ("model", include_str!("../model.wesl")),
        ("liquid", include_str!("../liquid.wesl")),
    ] {
        for definitions in [
            &[][..],
            &["PACKED_TERRAIN"][..],
            &["TERRAIN_UNITS"][..],
            &["ALPHA_TO_COVERAGE"][..],
        ] {
            let source = crate::shader_source::composed(source, definitions);
            let module = naga::front::wgsl::parse_str(&source).unwrap();
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
            for (version, environment) in [
                ((1, 0), "vulkan1.0"),
                ((1, 3), "vulkan1.1"),
                ((1, 5), "vulkan1.2"),
                ((1, 6), "vulkan1.3"),
            ] {
                for debug in [false, true] {
                    let mut flags = naga::back::spv::WriterFlags::LABEL_VARYINGS
                        | naga::back::spv::WriterFlags::FORCE_POINT_SIZE;
                    flags.set(naga::back::spv::WriterFlags::DEBUG, debug);
                    let options = naga::back::spv::Options {
                        lang_version: version,
                        flags,
                        bounds_check_policies: naga::proc::BoundsCheckPolicies {
                            index: naga::proc::BoundsCheckPolicy::Restrict,
                            buffer: naga::proc::BoundsCheckPolicy::Restrict,
                            image_load: naga::proc::BoundsCheckPolicy::Restrict,
                            binding_array: naga::proc::BoundsCheckPolicy::Unchecked,
                        },
                        ..Default::default()
                    };
                    for entry in &module.entry_points {
                        let pipeline = naga::back::spv::PipelineOptions {
                            shader_stage: entry.stage,
                            entry_point: entry.name.clone(),
                        };
                        let words =
                            naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline))
                                .unwrap();
                        assert_logical_allocations(&words);
                        if !available {
                            continue;
                        }
                        let label = format!(
                            "{name}-{}-{}-{debug}-{}",
                            definitions.join("_"),
                            environment,
                            entry.name
                        );
                        let path = output.path().join(format!("{label}.spv"));
                        write_words(&path, &words);
                        let result = Command::new(&validator)
                            .args(["--target-env", environment])
                            .arg(&path)
                            .output()
                            .unwrap();
                        assert!(
                            result.status.success(),
                            "{label}: {}",
                            String::from_utf8_lossy(&result.stderr)
                        );
                    }
                }
            }
        }
    }
}

/// Enforces Vulkan's allocation-layout rule independently of the installed validator version.
fn assert_logical_allocations(words: &[u32]) {
    use spirv::{Decoration, Op, StorageClass};
    if words[1] < 0x0001_0500 {
        return;
    }
    let mut children = BTreeMap::<u32, Vec<u32>>::new();
    let mut pointers = BTreeMap::new();
    let mut decorated = BTreeSet::new();
    let mut variables = Vec::new();
    let mut offset = 5;
    while offset < words.len() {
        let count = (words[offset] >> 16) as usize;
        assert!(
            count > 0 && count <= words.len() - offset,
            "invalid SPIR-V framing"
        );
        let instruction = &words[offset..offset + count];
        match instruction[0] & 0xffff {
            op if op == Op::TypeArray as u32 => {
                children.insert(instruction[1], vec![instruction[2]]);
            }
            op if op == Op::TypeStruct as u32 => {
                children.insert(instruction[1], instruction[2..].to_vec());
            }
            op if op == Op::TypePointer as u32 => {
                pointers.insert(instruction[1], instruction[3]);
            }
            op if op == Op::Decorate as u32 || op == Op::MemberDecorate as u32 => {
                let decoration = instruction[if op == Op::Decorate as u32 { 2 } else { 3 }];
                if [
                    Decoration::Block,
                    Decoration::BufferBlock,
                    Decoration::Offset,
                    Decoration::ArrayStride,
                    Decoration::MatrixStride,
                ]
                .iter()
                .any(|kind| *kind as u32 == decoration)
                {
                    decorated.insert(instruction[1]);
                }
            }
            op if op == Op::Variable as u32
                && [StorageClass::Function, StorageClass::Private]
                    .iter()
                    .any(|class| *class as u32 == instruction[3]) =>
            {
                variables.push(instruction[1]);
            }
            _ => {}
        }
        offset += count;
    }
    for pointer in variables {
        let mut pending = vec![pointers[&pointer]];
        let mut visited = BTreeSet::new();
        while let Some(ty) = pending.pop() {
            if !visited.insert(ty) {
                continue;
            }
            assert!(
                !decorated.contains(&ty),
                "logical allocation inherits a host memory layout"
            );
            if let Some(members) = children.get(&ty) {
                pending.extend(members);
            }
        }
    }
}

/// Stores SPIR-V in its required little-endian wire representation.
fn write_words(path: &Path, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    std::fs::write(path, bytes).unwrap();
}
