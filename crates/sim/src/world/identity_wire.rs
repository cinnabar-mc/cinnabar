//! The serialized form of world collision identities.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use world::{ChunkCollisionRevision, ChunkKey};

use super::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldIdentityWire {
    protocol: u32,
    id_space: CollisionIdSpace,
    preg_sha256: [u8; 32],
    chunks: Vec<ChunkRevisionWire>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkRevisionWire {
    dimension: i32,
    x: i32,
    z: i32,
    revision: u64,
}

impl Serialize for WorldCollisionIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WorldIdentityWire {
            protocol: self.registry.protocol,
            id_space: self.registry.id_space,
            preg_sha256: self.registry.preg_sha256,
            chunks: self
                .chunks
                .iter()
                .map(|revision| ChunkRevisionWire {
                    dimension: revision.chunk.dimension,
                    x: revision.chunk.x,
                    z: revision.chunk.z,
                    revision: revision.revision,
                })
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorldCollisionIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorldIdentityWire::deserialize(deserializer)?;
        Self::new(
            CollisionRegistryIdentity {
                protocol: wire.protocol,
                id_space: wire.id_space,
                preg_sha256: wire.preg_sha256,
            },
            wire.chunks
                .into_iter()
                .map(|revision| ChunkCollisionRevision {
                    chunk: ChunkKey::new(revision.dimension, revision.x, revision.z),
                    revision: revision.revision,
                }),
        )
        .map_err(D::Error::custom)
    }
}
