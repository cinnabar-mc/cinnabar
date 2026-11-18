use super::*;

impl GpuCull {
    /// Reads the records and enabled bits that this publication's compute cull consumes.
    pub(in crate::chunk::gpu_cull) fn publication_snapshot(
        &self,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Vec<Vec<u8>> {
        let Some(storage) = &self.storage else {
            return Vec::new();
        };
        let slots = u64::from(self.slot_count());
        let read = crate::render_test_support::read_buffer;
        vec![
            read(
                device,
                queue,
                &storage.records,
                0,
                slots * std::mem::size_of::<CullRecord>() as u64,
            ),
            read(device, queue, &storage.enabled, 0, slots.div_ceil(32) * 4),
        ]
    }
}

/// Storage that cannot grow writes nothing and retries.
#[test]
fn storage_without_memory_waits_until_it_grows() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
    let mut cull = GpuCull::new(&device, GpuCullSubmission::Fixed);
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label == "terrain cull storage");
        assert!(upload_records(&mut cull, &device, &queue).is_err());
    }
    assert!(cull.storage.is_none());
    assert!((0..8).any(|_| upload_records(&mut cull, &device, &queue).is_ok()));
    assert!(cull.storage.is_some());
}

/// Failed growth keeps old storage without publishing a prefix of the CPU table.
#[test]
fn failed_growth_keeps_storage_without_publishing_partial_records() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
    let mut cull = GpuCull::new(&device, GpuCullSubmission::Fixed);
    upload_records(&mut cull, &device, &queue).unwrap();
    let capacity = cull.storage.as_ref().expect("initial storage").capacity;
    cull.table.update(
        Entity::PLACEHOLDER,
        capacity + 10,
        ChunkBiomeTintIdentity::default(),
        CullRecord::default(),
        &HashSet::new(),
    );
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label == "terrain cull storage");
        assert!(upload_records(&mut cull, &device, &queue).is_err());
    }
    let held = cull.storage.as_ref().map(|storage| storage.capacity);
    assert_eq!(held, Some(capacity));
    assert_eq!(cull.slot_count(), capacity + 11);
    assert!((0..8).any(|_| upload_records(&mut cull, &device, &queue).is_ok()));
    assert_eq!(cull.slot_count(), capacity + 11);
}
