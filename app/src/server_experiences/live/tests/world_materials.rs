//! World material contributions: their application, and atomic revocation when a client part
//! stops.

use super::*;
use crate::server_experiences::world_materials::WorldMaterialPresenter;
use server_experience::world_material::{
    WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec, WorldTarget,
};

/// `bundle0` granted world materials in the `example` namespace, initialized with one material
/// bound to an entity and a block.
fn bound() -> Live<FakeWorker> {
    let mut live = fixture(1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::RenderWorldMaterial);
    instance.capabilities.namespace = Some("example".into());
    let material = WorldMaterialSpec {
        source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }".into(),
        textures: Vec::new(),
        params: Vec::new(),
        depth: WorldDepth::Test,
        blend: WorldBlend::Opaque,
    };
    let bind = |id, target| Command::WorldBinding {
        id,
        binding: Some(WorldBinding {
            target,
            material: 1,
        }),
    };
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner: instance.owner.clone(),
        epoch: live.epoch,
        commands: vec![
            Command::WorldMaterial {
                id: 1,
                material: Some(material),
            },
            bind(0, WorldTarget::Entity("example:moth".into())),
            bind(1, WorldTarget::Block("example:lamp".into())),
        ],
    });
    live.poll(1, 0, true, |_| true).unwrap();
    live
}

/// Rebuilds a presenter's set over budgets large enough for the fixture.
fn present(live: &Live<FakeWorker>, presenter: &mut WorldMaterialPresenter) -> bool {
    presenter.rebuild(live.world_bundles().map(|bundle| {
        crate::server_experiences::world_materials::BundleWorld {
            gpu_bytes: u64::MAX,
            memory_bytes: u64::MAX,
            ..bundle
        }
    }))
}

#[test]
fn a_stopped_client_part_revokes_its_world_materials_in_one_publication() {
    let mut live = bound();
    let mut presenter = WorldMaterialPresenter::default();
    assert!(present(&live, &mut presenter));
    let bound = std::sync::Arc::clone(presenter.set());
    assert!(bound.entity("example:moth").is_some() && bound.block("example:lamp").is_some());
    let revision = live.contribution_revision();
    // Repeated failures stop the part; its contributions go with it.
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = true;
    for strike in 1..=MAX_GUEST_STRIKES {
        receive(&mut live, "bundle0", strike as u64);
        let now = strike as u64 * CALLBACK_INTERVAL_MS;
        live.poll(1, now, true, |_| true).unwrap();
        live.poll(1, now, true, |_| true).unwrap();
    }
    assert!(live.instances["bundle0"].stopped.is_some());
    assert_ne!(live.contribution_revision(), revision);
    assert!(present(&live, &mut presenter));
    let revoked = presenter.set();
    // One publication removes the material and both bindings together.
    assert!(revoked.materials.is_empty());
    assert!(revoked.entities.is_empty() && revoked.blocks.is_empty());
    assert!(revoked.revision > bound.revision);
}

#[test]
fn a_transaction_binding_outside_the_namespace_is_refused_whole() {
    let mut live = fixture(1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::RenderWorldMaterial);
    instance.capabilities.namespace = Some("example".into());
    let mut contributions = Contributions::default();
    let transaction = Transaction {
        owner: instance.owner.clone(),
        epoch: 1,
        commands: vec![Command::WorldBinding {
            id: 0,
            binding: Some(WorldBinding {
                target: WorldTarget::Entity("minecraft:player".into()),
                material: 1,
            }),
        }],
    };
    assert!(
        contributions
            .apply(&transaction, &instance.owner, 1, &instance.capabilities)
            .is_err()
    );
    assert_eq!(contributions.world, Default::default());
}
