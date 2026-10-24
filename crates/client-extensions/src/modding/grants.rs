//! Explicit local environment grants for a selected personal component.

use super::packet_relay::PacketModSelection;
use mod_host::ModGrants;

const PLAYERS_ENV: &str = "CINNABAR_MOD_PLAYERS";
const PLAYER_STATE_ENV: &str = "CINNABAR_MOD_PLAYER_STATE";
const CLOCK_ENV: &str = "CINNABAR_MOD_CLOCK";
const ITEM_USE_ENV: &str = "CINNABAR_MOD_ITEM_USE";
const CAMERA_ENV: &str = "CINNABAR_MOD_CAMERA";
const CONTROLS_ENV: &str = "CINNABAR_MOD_CONTROLS";
const HUD_ENV: &str = "CINNABAR_MOD_HUD";
const MOVEMENT_ENV: &str = "CINNABAR_MOD_MOVEMENT";
const INTERACTION_ENV: &str = "CINNABAR_MOD_INTERACTION";
const SETTINGS_ENV: &str = "CINNABAR_MOD_SETTINGS";
const RENDER_ENV: &str = "CINNABAR_MOD_RENDER";
/// Lets a component create custom retained-drawing materials (`create-material`), beside the
/// world draw switch.
const RENDER_MATERIAL_ENV: &str = "CINNABAR_MOD_RENDER_MATERIAL";
const BLOCK_HIGHLIGHTS_ENV: &str = "CINNABAR_MOD_BLOCK_HIGHLIGHTS";
const FULLBRIGHT_ENV: &str = "CINNABAR_MOD_FULLBRIGHT";
const RENDER_DEPTH_ENV: &str = "CINNABAR_MOD_RENDER_DEPTH";
const ENTITIES_ENV: &str = "CINNABAR_MOD_ENTITIES";
/// A package's vanilla inventory requests, which its manifest must also ask for.
const INVENTORY_ENV: &str = "CINNABAR_MOD_INVENTORY";
/// Comma-separated command names the selected component may request.
const COMMANDS_ENV: &str = "CINNABAR_MOD_COMMANDS";
const PACKET_DELAY_ENV: &str = "CINNABAR_MOD_PACKET_DELAY";
const PACKET_DELAY_VISUAL_ENV: &str = "CINNABAR_MOD_PACKET_DELAY_VISUAL";

/// A packet mod component or core module selected on its own; it gets the packets grant.
pub const PACKET_MOD_ENV: &str = "CINNABAR_PACKET_MOD";
/// Also links WASI into the packet mod selected on its own.
const PACKET_MOD_WASI_ENV: &str = "CINNABAR_PACKET_MOD_WASI";

/// The packet mod selected on its own through the environment.
pub fn environment_packet_mod() -> Option<PacketModSelection> {
    let path = std::env::var_os(PACKET_MOD_ENV)?;
    Some(PacketModSelection {
        path: path.into(),
        grants: ModGrants {
            packets: true,
            wasi: std::env::var(PACKET_MOD_WASI_ENV).is_ok_and(|value| value == "1"),
            ..environment_grants()
        },
    })
}

/// The grants a component or package selected on its own gets from the environment.
pub fn environment_grants() -> ModGrants {
    grants_from(
        |name| std::env::var_os(name).is_some_and(|value| value == "1"),
        |name| std::env::var(name).ok(),
    )
}

/// The grants whose switches `enabled` reports set to `1`, with the command list `value`
/// returns for [`COMMANDS_ENV`].
fn grants_from(
    enabled: impl Fn(&str) -> bool,
    value: impl Fn(&str) -> Option<String>,
) -> ModGrants {
    ModGrants {
        environment: true,
        world_draw: enabled("CINNABAR_MOD_WORLD_DRAW"),
        render_through_walls: enabled("CINNABAR_MOD_RENDER_THROUGH_WALLS"),
        entity_observe: enabled("CINNABAR_MOD_ENTITY_OBSERVE"),
        entity_highlight: enabled("CINNABAR_MOD_ENTITY_HIGHLIGHT"),
        terrain: enabled("CINNABAR_MOD_TERRAIN"),
        players: enabled(PLAYERS_ENV),
        player_state: enabled(PLAYER_STATE_ENV),
        clock: enabled(CLOCK_ENV),
        item_use: enabled(ITEM_USE_ENV),
        camera: enabled(CAMERA_ENV),
        controls: enabled(CONTROLS_ENV),
        hud: enabled(HUD_ENV),
        movement: enabled(MOVEMENT_ENV),
        interaction: enabled(INTERACTION_ENV),
        settings: enabled(SETTINGS_ENV),
        render: enabled(RENDER_ENV),
        render_material: enabled(RENDER_MATERIAL_ENV),
        block_highlights: enabled(BLOCK_HIGHLIGHTS_ENV),
        fullbright: enabled(FULLBRIGHT_ENV),
        render_depth: enabled(RENDER_DEPTH_ENV),
        entities: enabled(ENTITIES_ENV),
        inventory: enabled(INVENTORY_ENV),
        commands: value(COMMANDS_ENV)
            .map(|names| {
                names
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        packet_delay: enabled(PACKET_DELAY_ENV) || enabled(PACKET_DELAY_VISUAL_ENV),
        ..ModGrants::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_render_material_switch_grants_custom_drawing_materials() {
        let grants = |names: &[&str]| grants_from(|name| names.contains(&name), |_| None);
        assert!(!grants(&["CINNABAR_MOD_WORLD_DRAW"]).render_material);
        let both = grants(&["CINNABAR_MOD_WORLD_DRAW", RENDER_MATERIAL_ENV]);
        assert!(both.world_draw && both.render_material);
    }
}
