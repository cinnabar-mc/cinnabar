//! The material showcase's server half: it owns the blocks the client part restyles, plain
//! cubes whose Bedrock textures show while their world materials are revoked or still
//! compiling. The local server drives the client part directly, so no callback stages anything.

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

/// The showcase's blocks: identifier suffix and display name. Each one's Bedrock texture is
/// `<suffix>.png`, which the `material-fx` tool generates.
pub const BLOCKS: [(&str, &str); 14] = [
    ("bottle", "Ship in a Bottle"),
    ("aquarium", "Hollow Aquarium"),
    ("mineshaft", "Hollow Mineshaft"),
    ("arena", "Arena Floor"),
    ("blackhole", "Black Hole"),
    ("canvas", "Living Painting"),
    ("glintore", "Breathing Diamond Ore"),
    ("pulseore", "Breathing Redstone Ore"),
    ("wallstone", "Corruptible Stone"),
    ("snowglobe", "Snow Globe"),
    ("lavalamp", "Lava Lamp"),
    ("screen", "Screen"),
    ("rune", "Carved Rune"),
    ("vortex", "Star Portal"),
];

impl Experience for MaterialFx {
    fn register() -> Result<Vec<BlockDef>, GuestError> {
        Ok(BLOCKS
            .iter()
            .map(|(name, display)| {
                block(
                    &format!("materialfx:{name}"),
                    display,
                    &format!("{name}.png"),
                )
            })
            .collect())
    }
}

impl experience_sdk::server::__private::Dispatch for MaterialFx {
    /// The client part sends nothing back; any record it might is ignored.
    fn dispatch(
        _ctx: &experience_sdk::server::Callback,
        _player: experience_sdk::server::PlayerId,
        _channel: String,
        _schema: u16,
        _payload: Vec<experience_sdk::Value>,
    ) -> Result<(), GuestError> {
        Ok(())
    }
}

experience_sdk::export_experience!(MaterialFx);
