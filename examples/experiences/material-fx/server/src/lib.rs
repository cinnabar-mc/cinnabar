//! The material showcase's server half: it owns the two blocks the client part restyles,
//! `materialfx:energy` and `materialfx:portal`, plain cubes whose Bedrock textures show while
//! their world materials are revoked or still compiling. The local server drives the client
//! part directly, so no callback stages anything.

use experience_sdk::server::{BlockDef, Experience, GuestError, Mining, TextureBinding};

struct MaterialFx;

/// A breakable cube `id` showing `texture` on every face.
fn block(id: &str, display_name: &str, texture: &str) -> BlockDef {
    BlockDef {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        textures: vec![TextureBinding {
            slot: "*".to_owned(),
            path: texture.to_owned(),
        }],
        mining: Mining::Breakable(1.0),
    }
}

impl Experience for MaterialFx {
    fn register() -> Result<Vec<BlockDef>, GuestError> {
        Ok(vec![
            block("materialfx:energy", "Energy Conduit", "energy.png"),
            block("materialfx:portal", "Star Portal", "portal.png"),
        ])
    }
}

experience_sdk::export_experience!(MaterialFx);
