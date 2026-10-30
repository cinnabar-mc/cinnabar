use assets::NetworkIdMode;
use protocol::ParticleEvent;
use protocol_ids::LevelEvent;

use super::WorldAuthority;

impl WorldAuthority {
    /// Keeps terrain event ids in the same palette as decoded blocks.
    pub fn remap_particle_block_ids(&self, event: &mut ParticleEvent) {
        if self.network_id_mode != NetworkIdMode::Sequential || self.id_remap.is_identity() {
            return;
        }
        let ParticleEvent::Level(level) = event else {
            return;
        };
        let data = level.data as u32;
        level.data = match level.event_id {
            2001 | 2021 | 0x4013 => self.id_remap.to_internal(data) as i32,
            id if id == LevelEvent::ParticlesCrackBlock.wire_value() => {
                let id = self.id_remap.to_internal(data & 0x00ff_ffff);
                ((data & 0xff00_0000) | (id & 0x00ff_ffff)) as i32
            }
            id if (LevelEvent::ParticlesCrackBlockDown.wire_value()
                ..=LevelEvent::ParticlesCrackBlockEast.wire_value())
                .contains(&id) =>
            {
                self.id_remap.to_internal(data) as i32
            }
            _ => level.data,
        };
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use assets::{RuntimeAssets, SequentialIdRemap};
    use protocol::{LevelParticleEvent, WorldBootstrap};

    use super::*;

    /// Creates an authority whose wire palette inserts two states before vanilla id three.
    fn authority(hashes: bool) -> WorldAuthority {
        let mut authority = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: hashes,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            None,
            [0.0; 3],
            None,
        );
        authority.set_sequential_id_remap(SequentialIdRemap::new([(3, 2, 10)]));
        authority
    }

    #[test]
    fn face_cracks_remap_the_full_sequential_id_and_preserve_hashes() {
        for hashes in [false, true] {
            let authority = authority(hashes);
            for event_id in LevelEvent::ParticlesCrackBlockDown.wire_value()
                ..=LevelEvent::ParticlesCrackBlockEast.wire_value()
            {
                for (wire, internal) in [(7, 5), (3, 10), (0xfabc_def0_u32, 0xfabc_deee)] {
                    let mut event = ParticleEvent::Level(LevelParticleEvent {
                        event_id,
                        position: [0.0; 3],
                        data: wire as i32,
                    });
                    authority.remap_particle_block_ids(&mut event);
                    let ParticleEvent::Level(level) = event else {
                        unreachable!();
                    };
                    assert_eq!(level.event_id, event_id);
                    assert_eq!(level.data as u32, if hashes { wire } else { internal });
                }
            }
        }
    }

    #[test]
    fn packed_cracks_keep_face_bits_while_remapping_the_id() {
        let authority = authority(false);
        for face in 0..6 {
            let mut event = ParticleEvent::Level(LevelParticleEvent {
                event_id: LevelEvent::ParticlesCrackBlock.wire_value(),
                position: [0.0; 3],
                data: (face << 24) | 7,
            });
            authority.remap_particle_block_ids(&mut event);
            let ParticleEvent::Level(level) = event else {
                unreachable!();
            };
            assert_eq!(level.data, (face << 24) | 5);
        }
    }
}
