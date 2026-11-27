use super::*;
use bevy::ecs::system::RunSystemOnce;

/// An empty bind group from a headless device, or `None` without an adapter.
fn empty_group() -> Option<BindGroup> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("missing fixture: headless GPU for fused composite binding tests");
            return None;
        }
        Err(error) => panic!("fused composite adapter: {error}"),
    };
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&Default::default()))
        .expect("fused composite device");
    let device = RenderDevice::from(device);
    let layout = device.create_bind_group_layout(None, &[]);
    Some(device.create_bind_group(None, &layout, &[]))
}

#[test]
fn a_view_that_stops_fusing_releases_its_retired_targets() {
    let Some(group) = empty_group() else {
        return;
    };
    let mut world = World::new();
    world.init_resource::<FusedComposite>();
    let (drawn, idle) = (world.spawn_empty().id(), world.spawn_empty().id());
    {
        let fused = world.resource::<FusedComposite>();
        let mut bindings = fused.bindings.lock().unwrap();
        for view in [drawn, idle] {
            bindings.insert(
                view,
                FusedBinding {
                    targets: [TextureViewId::new(), TextureViewId::new()],
                    group: group.clone(),
                    used: true,
                },
            );
        }
    }
    world.run_system_once(retire_unused_bindings).unwrap();
    world
        .resource::<FusedComposite>()
        .bindings
        .lock()
        .unwrap()
        .get_mut(&drawn)
        .unwrap()
        .used = true;
    world.run_system_once(retire_unused_bindings).unwrap();
    let fused = world.resource::<FusedComposite>();
    let bindings = fused.bindings.lock().unwrap();
    assert!(
        bindings.contains_key(&drawn),
        "a binding drawn every frame stays cached"
    );
    assert!(
        !bindings.contains_key(&idle),
        "a binding no frame drew must release its layer and scene targets"
    );
}
