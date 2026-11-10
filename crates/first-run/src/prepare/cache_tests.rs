use std::os::unix::fs::PermissionsExt;

use assets::{VanillaSource, carriers::Sources};
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    test_support::{Dir, pack_archive, write_vanilla_manifest},
    tests::installed_layout,
};

/// A compiler replacement that rebuilds required outputs from the cached source pack.
fn install_compiler(layout: &InstallLayout, generation: &str) {
    let mut script = String::from(
        "#!/bin/sh\nset -eu\nwhile [ \"$#\" -gt 0 ]; do\n\
         case \"$1\" in\n\
         --out) out=\"$2\"; shift;;\n\
         --check) echo '{\"stale\":[\"world\"],\"needs_pack\":true}'; exit 0;;\n\
         esac\nshift\ndone\n",
    );
    let pack = format!(".local/{}", vanilla_pack_relative());
    script.push_str(&format!(
        "test -f '{pack}/blocks.json'\ntest -f '{pack}/../behavior_pack/entities/fixture.json'\n"
    ));
    for carrier in carriers::required() {
        script.push_str(&format!(
            "printf '%s' '{generation}' > \"$out/{}\"\n",
            carrier.output
        ));
    }
    let compiler = carriers::kit_compiler(&layout.prep_kit());
    fs::create_dir_all(compiler.parent().unwrap()).unwrap();
    fs::write(&compiler, script).unwrap();
    fs::set_permissions(compiler, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_new_client_compiler_rebuilds_offline_from_the_cached_resource_version() {
    let data = Dir::new("compiler-update-cache");
    let mut layout = installed_layout(&data, "/opt/cinnabar/bin/cinnabar");
    layout.resource_root = data.path().join("resources");
    let kit = layout.prep_kit();
    let workspace = layout.prepare_workspace();
    let archive = pack_archive(b"texture");
    write_vanilla_manifest(
        &kit,
        "https://127.0.0.1:9/unavailable.zip",
        &format!("{:x}", Sha256::digest(&archive)),
        "pack.zip",
    );
    let manifest = Sources::Kit(kit.clone()).resolve(carriers::VANILLA_MANIFEST);
    let mut source: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    source["cache_dir"] = assets::vanilla_source().cache_dir.to_string().into();
    fs::write(&manifest, source.to_string()).unwrap();
    let paths = VanillaSource::read(&manifest)
        .unwrap()
        .local_paths(&workspace)
        .unwrap();
    fs::create_dir_all(paths.archive.parent().unwrap()).unwrap();
    fs::write(&paths.archive, archive).unwrap();
    install_compiler(&layout, "first");
    let cancel = AtomicBool::new(false);
    prepare(&layout, &cancel, &mut |_| {}).unwrap();
    assert!(paths.cache.join("resource_pack/blocks.json").is_file());

    // An app upgrade can replace the compiler and the archive can be evicted independently.
    fs::remove_file(paths.archive).unwrap();
    fs::remove_dir_all(layout.vanilla_pack_dir()).unwrap();
    install_compiler(&layout, "second");
    let mut reports = Vec::new();
    prepare(&layout, &cancel, &mut |status| reports.push(status)).unwrap();
    assert!(
        reports
            .iter()
            .all(|status| status.phase != Phase::Downloading)
    );
    assert!(reports.iter().all(|status| status.label != UNPACK_LABEL));
    assert!(vanilla_textures_kept(&layout));
    for carrier in carriers::required() {
        assert_eq!(
            fs::read(layout.prepared_assets_dir().join(carrier.output)).unwrap(),
            b"second"
        );
    }
}
