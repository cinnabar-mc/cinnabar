//! The material showcase's server half: it owns the blocks the client part restyles, plain
//! cubes whose Bedrock textures show while their world materials are revoked or still
//! compiling. The glass pieces blend, so the world behind them shows as it does through glass.
//! The local server drives the client part directly, so no callback stages anything.

use experience_sdk::server::{
    BlockDef, BlockType, Experience, GuestError, Material, Mining, PlacementStates, Registration,
    RenderMethod, TextureBinding, Visual,
};

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

/// The blocks whose world materials blend: glass the world behind shows through, so their
/// Bedrock looks blend too and hide none of their neighbours' faces.
pub const BLENDED: [&str; 3] = ["bottle", "snowglobe", "blackhole"];

/// The block type of showcase block `name`: a plain cube, or for a [`BLENDED`] block a full
/// cube whose one material blends.
fn block_type(name: &str, display_name: &str) -> BlockType {
    let id = format!("materialfx:{name}");
    let texture = format!("{name}.png");
    if !BLENDED.contains(&name) {
        return experience_sdk::server::cube(block(&id, display_name, &texture));
    }
    let mut def = block(&id, display_name, &texture);
    def.textures.clear();
    BlockType {
        def,
        states: Vec::new(),
        placement: PlacementStates::empty(),
        visual: Some(Visual {
            geometry: None,
            materials: vec![Material {
                instance: "*".to_owned(),
                path: texture,
                render_method: RenderMethod::Blend,
                flipbook: None,
            }],
            bones: Vec::new(),
            collision: None,
            selection: None,
            rotation: None,
        }),
        permutations: Vec::new(),
        network: false,
    }
}

impl Experience for MaterialFx {
    fn registration() -> Result<Registration, GuestError> {
        Ok(Registration {
            blocks: BLOCKS
                .iter()
                .map(|(name, display)| block_type(name, display))
                .collect(),
            items: Vec::new(),
        })
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
