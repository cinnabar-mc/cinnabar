//! Immutable particle list identities and their last uploaded offsets.

use super::*;

#[derive(Default)]
pub(super) struct InstanceUpload {
    lists: [Arc<[ParticleInstance]>; 3],
    offsets: [usize; 3],
    pub capacity: render_model::CapacityRetention,
}

impl InstanceUpload {
    /// Selects changed live ranges; replacement storage must receive every live byte.
    pub fn changes(&self, frame: &ParticleGpuFrame, replaced: bool) -> [bool; 3] {
        let offsets = [
            0,
            frame.opaque.len(),
            frame.opaque.len() + frame.blend.len(),
        ];
        std::array::from_fn(|index| {
            let list = [&frame.opaque, &frame.blend, &frame.add][index];
            !list.is_empty()
                && (replaced
                    || offsets[index] != self.offsets[index]
                    || !Arc::ptr_eq(list, &self.lists[index]))
        })
    }

    /// Keeps the exact list owners alive until a later upload replaces their identity.
    pub fn accept(&mut self, frame: &ParticleGpuFrame) {
        self.lists = [
            Arc::clone(&frame.opaque),
            Arc::clone(&frame.blend),
            Arc::clone(&frame.add),
        ];
        self.offsets = [
            0,
            frame.opaque.len(),
            frame.opaque.len() + frame.blend.len(),
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_uploads_reconstruct_every_live_byte_through_moves_and_replacement() {
        let mut state = InstanceUpload::default();
        let mut frame = ParticleGpuFrame {
            opaque: vec![ParticleInstance::default(); 8].into(),
            blend: vec![ParticleInstance::default(); 3].into(),
            add: vec![ParticleInstance::default(); 2].into(),
            ..Default::default()
        };
        let mut gpu = Vec::new();
        for step in 0..9 {
            let replaced = matches!(step, 0 | 7);
            match step {
                2 => frame.opaque = vec![ParticleInstance::default(); 2].into(),
                3 => Arc::make_mut(&mut frame.blend)[0].center_light[0] = -0.0,
                4 => Arc::make_mut(&mut frame.add)[1].color[0] = f32::from_bits(0x7fc00001),
                5 => frame.opaque = Arc::from([]),
                6 => frame.base = Some(Arc::from([255; 4])),
                8 => {
                    frame.opaque = Arc::from([]);
                    frame.blend = Arc::from([]);
                    frame.add = Arc::from([]);
                }
                _ => {}
            }
            if replaced {
                gpu = vec![0xcd; frame.instance_count() * INSTANCE_BYTES as usize];
            }
            gpu.resize(frame.instance_count() * INSTANCE_BYTES as usize, 0xcd);
            let changes = state.changes(&frame, replaced);
            if matches!(step, 1 | 6) {
                assert_eq!(changes, [false; 3]);
            }
            if step == 2 {
                assert_eq!(changes, [true; 3]);
            }
            if step == 3 {
                assert_eq!(changes, [false, true, false]);
            }
            let mut offset = 0;
            for (index, list) in [&frame.opaque, &frame.blend, &frame.add]
                .into_iter()
                .enumerate()
            {
                let bytes = bytemuck::cast_slice::<_, u8>(list);
                if changes[index] {
                    gpu[offset..offset + bytes.len()].copy_from_slice(bytes);
                }
                offset += bytes.len();
            }
            let expected: Vec<_> = [&frame.opaque, &frame.blend, &frame.add]
                .into_iter()
                .flat_map(|list| bytemuck::cast_slice::<_, u8>(list).iter().copied())
                .collect();
            assert_eq!(gpu, expected);
            state.accept(&frame);
            assert_eq!(state.changes(&frame, false), [false; 3]);
        }
    }
}
