use bevy::{
    app::SubApp,
    prelude::*,
    render::{
        Render, RenderSystems,
        camera::ExtractedCamera,
        renderer::RenderDevice,
        view::{
            ViewTarget,
            window::{ExtractedWindow, SurfaceData, create_surfaces},
        },
    },
};

/// Releases inactive and suspended camera outputs before Bevy repairs a window surface.
pub(crate) fn install(render_app: &mut SubApp) {
    render_app.add_systems(
        Render,
        (
            release_orphan_targets::<ViewTarget>,
            release_retained_targets::<ViewTarget>.run_if(surface_repair_pending),
        )
            .in_set(RenderSystems::PrepareViews)
            .before(create_surfaces),
    );
}

/// A lost surface needs every old output released, including outputs of retargeted cameras.
fn surface_repair_pending(
    surfaces: Query<(&ExtractedWindow, &SurfaceData)>,
    device: Res<RenderDevice>,
) -> bool {
    surfaces.iter().any(|(window, surface)| {
        window.is_drawable(device.limits().max_texture_dimension_2d)
            && surface.needs_reconfiguration()
    })
}

/// Drops all retained outputs before repair, since a camera may now name a different target.
fn release_retained_targets<T: Component>(mut commands: Commands, targets: Query<Entity, With<T>>) {
    for entity in &targets {
        commands.entity(entity).remove::<T>();
    }
}

/// Inactive, zero-size and oversized cameras lose `ExtractedCamera` during extraction, but Bevy
/// retains their `ViewTarget`. Its output view must drop before DXGI resizes the window.
/// Bevy's ordinary resize cleanup only sees targets with an extracted camera.
fn release_orphan_targets<T: Component>(
    mut commands: Commands,
    targets: Query<Entity, (With<T>, Without<ExtractedCamera>)>,
) {
    for entity in &targets {
        commands.entity(entity).remove::<T>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::{CameraOutputMode, ClearColorConfig, MsaaWriteback},
        core_pipeline::Core3d,
        ecs::schedule::ScheduleLabel,
    };
    use std::sync::{Arc, Weak};

    #[derive(Component)]
    struct RetainedTarget(
        #[expect(dead_code, reason = "retains the surface ownership fixture")] Arc<()>,
    );

    #[derive(Resource)]
    struct SurfaceOwner(Weak<()>);

    /// Asserts that deferred removal released every retained output before configuration.
    fn configure_surface(owner: Res<SurfaceOwner>) {
        assert!(
            owner.0.upgrade().is_none(),
            "resize retained the old surface view"
        );
    }

    /// Builds an extracted camera without allocating GPU resources.
    fn camera() -> ExtractedCamera {
        ExtractedCamera {
            target: None,
            physical_viewport_size: None,
            physical_target_size: None,
            viewport: None,
            schedule: Core3d.intern(),
            order: 0,
            output_mode: CameraOutputMode::default(),
            msaa_writeback: MsaaWriteback::default(),
            clear_color: ClearColorConfig::default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: false,
        }
    }

    #[test]
    fn camera_deactivation_releases_surface_views_before_reconfiguration() {
        let mut world = World::new();
        let retained = Arc::new(());
        world.insert_resource(SurfaceOwner(Arc::downgrade(&retained)));
        let entity = world.spawn((RetainedTarget(retained), camera())).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(release_orphan_targets::<RetainedTarget>);
        schedule.run(&mut world);
        assert!(world.entity(entity).contains::<RetainedTarget>());

        world.entity_mut(entity).remove::<ExtractedCamera>();
        schedule.add_systems(configure_surface.after(release_orphan_targets::<RetainedTarget>));
        schedule.run(&mut world);
        assert!(!world.entity(entity).contains::<RetainedTarget>());
    }

    #[test]
    fn repair_releases_active_and_retargeted_outputs_before_reconfiguration() {
        let mut world = World::new();
        let retained = Arc::new(());
        world.insert_resource(SurfaceOwner(Arc::downgrade(&retained)));
        let active = world
            .spawn((RetainedTarget(retained.clone()), camera()))
            .id();
        let mut changed_target = camera();
        changed_target.target = Some(bevy::camera::NormalizedRenderTarget::TextureView(
            bevy::camera::ManualTextureViewHandle(0),
        ));
        let retargeted = world.spawn((RetainedTarget(retained), changed_target)).id();
        let mut schedule = Schedule::default();
        schedule.add_systems((
            release_retained_targets::<RetainedTarget>,
            configure_surface.after(release_retained_targets::<RetainedTarget>),
        ));
        schedule.run(&mut world);
        assert!(!world.entity(active).contains::<RetainedTarget>());
        assert!(!world.entity(retargeted).contains::<RetainedTarget>());
        assert!(world.entity(active).contains::<ExtractedCamera>());
        assert!(world.entity(retargeted).contains::<ExtractedCamera>());
    }

    #[test]
    fn native_gpu_orphan_target_survives_bevy_resize_cleanup_and_is_released() {
        use bevy::{
            camera::{CameraMainTextureUsages, NormalizedRenderTarget, RenderTarget},
            render::{
                render_resource::TextureView,
                renderer::RenderDevice,
                texture::{OutputColorAttachment, TextureCache},
                view::{
                    ExtractedView, Msaa, RetainedViewEntity, ViewTargetAttachments,
                    cleanup_view_targets_for_resize, prepare_view_targets,
                },
            },
        };
        const SIDE: u32 = 8;
        let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture(
            "native_gpu_orphan_target_survives_bevy_resize_cleanup_and_is_released",
        ) else {
            return;
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("surface ownership regression"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let output = TextureView::from(texture.create_view(&Default::default()));
        let target: NormalizedRenderTarget =
            RenderTarget::Window(bevy::window::WindowRef::Entity(Entity::PLACEHOLDER))
                .normalize(None)
                .unwrap();
        let mut attachments = ViewTargetAttachments::default();
        attachments.insert(
            target.clone(),
            OutputColorAttachment::new(output, texture.format()),
        );
        let mut world = World::new();
        world.insert_resource(RenderDevice::from(gpu.device));
        world.insert_resource(TextureCache::default());
        world.insert_resource(ClearColor::default());
        world.insert_resource(attachments);
        let mut camera = camera();
        camera.target = Some(target);
        camera.physical_target_size = Some(UVec2::splat(SIDE));
        let entity = world
            .spawn((
                camera,
                ExtractedView {
                    retained_view_entity: RetainedViewEntity::new(
                        Entity::PLACEHOLDER.into(),
                        None,
                        0,
                    ),
                    clip_from_view: Mat4::IDENTITY,
                    world_from_view: GlobalTransform::default(),
                    clip_from_world: None,
                    target_format: render_scene::SCENE_COLOR_FORMAT,
                    viewport: UVec4::new(0, 0, SIDE, SIDE),
                    color_grading: Default::default(),
                    invert_culling: false,
                },
                CameraMainTextureUsages::default(),
                Msaa::Off,
            ))
            .id();
        let mut prepare = Schedule::default();
        prepare.add_systems(prepare_view_targets);
        prepare.run(&mut world);
        assert!(world.entity(entity).contains::<ViewTarget>());
        world.resource_mut::<ViewTargetAttachments>().clear();
        world.entity_mut(entity).remove::<ExtractedCamera>();
        let mut bevy_cleanup = Schedule::default();
        bevy_cleanup.add_systems(cleanup_view_targets_for_resize);
        bevy_cleanup.run(&mut world);
        assert!(
            world.entity(entity).contains::<ViewTarget>(),
            "pinned Bevy retains the orphan"
        );
        let mut cleanup = Schedule::default();
        cleanup.add_systems(release_orphan_targets::<ViewTarget>);
        cleanup.run(&mut world);
        assert!(!world.entity(entity).contains::<ViewTarget>());
    }
}
