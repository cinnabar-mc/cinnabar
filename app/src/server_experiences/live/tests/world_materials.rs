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
    instance.capabilities.scope.memory_bytes = server_experience::policy::MAX_GUEST_MEMORY;
    instance.capabilities.scope.gpu_bytes = u64::MAX;
    let material = WorldMaterialSpec {
        source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }".into(),
        textures: Vec::new(),
        params: Vec::new(),
        depth: WorldDepth::Test,
        blend: WorldBlend::Opaque,
        effects: Default::default(),
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
    live.poll_ready(1, 0).unwrap();
    live
}

/// No permission withdrawn.
static NO_WITHDRAWALS: BTreeSet<server_experience::manifest::Permission> = BTreeSet::new();

/// Rebuilds a presenter's set over budgets large enough for the fixture.
fn present(live: &Live<FakeWorker>, presenter: &mut WorldMaterialPresenter) -> bool {
    presenter.rebuild(live.world_bundles(&NO_WITHDRAWALS).map(|bundle| {
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
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(present(&live, &mut presenter));
    let bound = std::sync::Arc::clone(presenter.set());
    assert!(bound.entity("example:moth").is_some() && bound.block("example:lamp").is_some());
    let revision = live.world_contribution();
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
        live.poll_ready(1, now).unwrap();
        live.poll_ready(1, now).unwrap();
    }
    assert!(live.instances["bundle0"].stopped.is_some());
    assert_ne!(live.world_contribution(), revision);
    assert!(present(&live, &mut presenter));
    let revoked = presenter.set();
    // One publication removes the material and both bindings together.
    assert!(revoked.published().next().is_none());
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

/// Completes one message callback on `bundle0` whose transaction carries `commands`.
fn callback(live: &mut Live<FakeWorker>, sequence: u64, commands: Vec<Command>) {
    receive(live, "bundle0", sequence);
    let now = sequence * CALLBACK_INTERVAL_MS;
    live.poll(1, now, true, |_| true).unwrap();
    let epoch = live.epoch;
    let instance = live.instances.get_mut("bundle0").unwrap();
    let owner = instance.owner.clone();
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner,
        epoch,
        commands,
    });
    live.poll(1, now, true, |_| true).unwrap();
}

/// A world material declaration of `bundle0`'s material 1 with its own program per `round`.
fn redeclare(round: u32) -> Command {
    Command::WorldMaterial {
        id: 1,
        material: Some(WorldMaterialSpec {
            source: format!(
                "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({round}.0), 1.0, vec3<f32>(0.0)); }}"
            ),
            textures: Vec::new(),
            params: Vec::new(),
            depth: WorldDepth::Test,
            blend: WorldBlend::Opaque,
            effects: Default::default(),
        }),
    }
}

#[test]
fn only_world_commands_and_stops_change_the_world_contribution() {
    let mut live = bound();
    let built = live.world_contribution();
    callback(&mut live, 1, Vec::new());
    assert_eq!(
        live.world_contribution(),
        built,
        "a callback without world commands leaves the presenter idle"
    );
    callback(&mut live, 2, vec![redeclare(1)]);
    let redeclared = live.world_contribution();
    assert_ne!(redeclared, built);
    live.instances.get_mut("bundle0").unwrap().stop(
        &mut live.budget,
        &mut live.scene_revision,
        "for the test",
    );
    assert_ne!(live.world_contribution(), redeclared);
    assert_ne!(
        bound().world_contribution().0,
        built.0,
        "each runtime has its own identity"
    );
}

#[test]
fn a_new_runtime_starts_without_the_last_sessions_retired_charges() {
    use crate::server_experiences::world_materials::{
        BIND_GROUP_CHARGE_BYTES, PIPELINE_CHARGE_BYTES,
    };
    let budget = PIPELINE_CHARGE_BYTES + BIND_GROUP_CHARGE_BYTES;
    let mut live = bound();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .capabilities
        .scope
        .gpu_bytes = budget;
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new()));
    assert!(presenter.set().entity("example:moth").is_some());
    assert!(
        !presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new()),
        "nothing changed"
    );
    // Replacing the drawn material retires its pipeline, which uses up the share.
    callback(&mut live, 1, vec![redeclare(1)]);
    presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new());
    assert!(presenter.set().entity("example:moth").is_none());
    // The session ends and the same bundle reconnects with the same share.
    let mut again = bound();
    again
        .instances
        .get_mut("bundle0")
        .unwrap()
        .capabilities
        .scope
        .gpu_bytes = budget;
    assert!(presenter.follow(Some(&mut again), (0, u64::MAX), &mut BTreeSet::new()));
    assert!(presenter.set().entity("example:moth").is_some());
}

#[test]
fn new_parameters_patch_the_published_ones_without_a_new_set() {
    let mut live = bound();
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new()));
    let set = std::sync::Arc::clone(presenter.set());
    let key = set.entity("example:moth").unwrap().1.key;
    let flash = |value: f32| Command::WorldMaterial {
        id: 1,
        material: Some(WorldMaterialSpec {
            source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }"
                .into(),
            textures: Vec::new(),
            params: vec![value],
            depth: WorldDepth::Test,
            blend: WorldBlend::Opaque,
            effects: Default::default(),
        }),
    };
    let structure = live.world_contribution();
    callback(&mut live, 1, vec![flash(0.75)]);
    assert_eq!(
        live.world_contribution(),
        structure,
        "a parameter-only update is no structural change"
    );
    assert!(presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new()));
    assert!(
        std::sync::Arc::ptr_eq(&set, presenter.set()),
        "the set is not rebuilt or republished"
    );
    assert_eq!(presenter.params().values[&key][0], 0.75);
    assert!(
        !presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new()),
        "nothing more"
    );
}

#[test]
fn the_retained_declarations_and_decoded_textures_share_one_memory_ledger() {
    let mut live = bound();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.files = std::sync::Arc::new(server_experience::screen::Files {
        namespace: "example".into(),
        templates: Default::default(),
        textures: vec![("textures/glow.png".to_owned(), {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            bytes.into_inner()
        })],
    });
    let source = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }";
    let textured = WorldMaterialSpec {
        source: source.into(),
        textures: vec!["textures/glow.png".into()],
        params: Vec::new(),
        depth: WorldDepth::Test,
        blend: WorldBlend::Opaque,
        effects: Default::default(),
    };
    instance
        .capabilities
        .assets
        .insert("textures/glow.png".to_owned());
    callback(
        &mut live,
        1,
        vec![Command::WorldMaterial {
            id: 1,
            material: Some(textured),
        }],
    );
    // Room for the declarations and the decoded texels on their own, but not for both.
    let instance = live.instances.get_mut("bundle0").unwrap();
    let declarations = instance.contributions.world.charge();
    instance.capabilities.scope.memory_bytes = declarations + 8 * 8 * 4 + source.len() as u64 - 1;
    let mut presenter = WorldMaterialPresenter::inline();
    presenter.follow(Some(&mut live), (0, u64::MAX), &mut BTreeSet::new());
    assert!(presenter.set().entity("example:moth").is_none());
    assert!(
        presenter
            .diagnostics()
            .any(|line| line.contains("memory limit"))
    );
}

#[test]
fn a_refused_transaction_undoes_its_world_changes_made_in_place() {
    let mut live = bound();
    let instance = live.instances.get_mut("bundle0").unwrap();
    let held = instance.contributions.world.clone();
    let transaction = Transaction {
        owner: instance.owner.clone(),
        epoch: live.epoch,
        commands: vec![
            redeclare(9),
            Command::WorldBinding {
                id: 0,
                binding: None,
            },
            Command::WorldBinding {
                id: 7,
                binding: Some(WorldBinding {
                    target: WorldTarget::Entity("minecraft:player".into()),
                    material: 1,
                }),
            },
        ],
    };
    assert!(
        instance
            .contributions
            .apply(
                &transaction,
                &instance.owner,
                live.epoch,
                &instance.capabilities
            )
            .is_err()
    );
    assert_eq!(
        instance.contributions.world, held,
        "the material and the unbinding applied before the refusal are undone"
    );
}

#[test]
fn ending_a_session_gives_back_every_withdrawn_permission() {
    let mut live = bound();
    let mut presenter = WorldMaterialPresenter::inline();
    let mut withdrawn = BTreeSet::new();
    presenter.follow(Some(&mut live), (0, u64::MAX), &mut withdrawn);
    withdrawn.insert(server_experience::manifest::Permission::RenderWorldMaterialVertex);
    presenter.follow(Some(&mut live), (0, u64::MAX), &mut withdrawn);
    assert!(!withdrawn.is_empty(), "the running session keeps them");
    let mut again = bound();
    presenter.follow(Some(&mut again), (0, u64::MAX), &mut withdrawn);
    assert!(
        withdrawn.is_empty(),
        "a new runtime starts with none withdrawn"
    );
    withdrawn.insert(server_experience::manifest::Permission::RenderWorldMaterialScene);
    presenter.follow::<FakeWorker>(None, (0, u64::MAX), &mut withdrawn);
    assert!(withdrawn.is_empty(), "and so does stopping the runtime");
}
