use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::IntoScheduleConfigs;
use bevy::render::renderer::RenderQueue;

/// Preparation clears the uploaded labels once after the last one leaves, then idles.
#[test]
fn empty_scenes_skip_label_preparation_after_clearing_the_last_records() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.insert_resource(NametagSceneResource::default());
    assert!(
        world.run_system_once(nametags_due).unwrap(),
        "preparation runs until the GPU exists"
    );
    world.run_system_once(init_nametag_gpu).unwrap();
    assert!(
        !world.run_system_once(nametags_due).unwrap(),
        "an empty scene starts idle"
    );
    assert!(!world.run_system_once(nametags_uploaded).unwrap());
    let mut schedule = bevy::ecs::schedule::Schedule::default();
    schedule.add_systems(prepare_nametags.run_if(nametags_due));

    world.resource_mut::<NametagSceneResource>().records = vec![NametagRecord::default()];
    schedule.run(&mut world);
    assert_eq!(world.resource::<NametagGpu>().total, 1);
    assert!(world.run_system_once(nametags_uploaded).unwrap());

    world.resource_mut::<NametagSceneResource>().records.clear();
    assert!(
        world.run_system_once(nametags_due).unwrap(),
        "the clearing pass still runs"
    );
    schedule.run(&mut world);
    assert_eq!(world.resource::<NametagGpu>().total, 0);
    assert!(!world.run_system_once(nametags_due).unwrap());
    assert!(!world.run_system_once(nametags_uploaded).unwrap());
}
