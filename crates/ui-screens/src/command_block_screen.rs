//! The command block editor through vanilla `command_block.command_block_screen`: the bindings its
//! screen controller supplies, computed from the block being edited.

use std::collections::BTreeMap;

use json_ui::{DataSource, Scalar, ViewState};
use serde_json::Value;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame, translated},
};
use ui_engine::frame::EngineFrame;

pub const COMMAND_BLOCK_SCREEN: &str = "command_block.command_block_screen";

/// A command block's behaviour, numbered as the block and the update packet number it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommandBlockMode {
    #[default]
    Impulse,
    Repeat,
    Chain,
}

impl CommandBlockMode {
    /// The mode's wire value.
    #[must_use]
    pub const fn wire_value(self) -> u32 {
        match self {
            Self::Impulse => 0,
            Self::Repeat => 1,
            Self::Chain => 2,
        }
    }

    /// The mode a wire value names; unknown values read as impulse.
    #[must_use]
    pub const fn from_wire(value: i64) -> Self {
        match value {
            1 => Self::Repeat,
            2 => Self::Chain,
            _ => Self::Impulse,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Impulse => "commandBlockScreen.blockType.impulse",
            Self::Repeat => "commandBlockScreen.blockType.repeat",
            Self::Chain => "commandBlockScreen.blockType.chain",
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::Impulse => "textures/ui/ImpulseSquare",
            Self::Repeat => "textures/ui/RepeatSquare",
            Self::Chain => "textures/ui/ChainSquare",
        }
    }

    const fn base_color(self) -> Option<[f64; 4]> {
        match self {
            Self::Impulse => Some([0.89, 0.529, 0.29, 1.0]),
            Self::Repeat => Some([0.32, 0.239, 0.6, 1.0]),
            Self::Chain => None,
        }
    }

    /// The previous-output label's color: chain reads lighter there than on the dropdown.
    fn previous_color(self) -> [f64; 4] {
        self.base_color().unwrap_or([0.631, 0.765, 0.706, 1.0])
    }

    fn dropdown_color(self) -> [f64; 4] {
        self.base_color().unwrap_or([0.227, 0.447, 0.333, 1.0])
    }
}

/// The modes the block last performed with, shown beside its previous output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PerformedModes {
    pub mode: CommandBlockMode,
    pub conditional: bool,
    pub needs_redstone: bool,
}

/// Which dropdowns are expanded.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpenDropdowns {
    pub block_type: bool,
    pub condition: bool,
    pub redstone: bool,
}

/// What one paint of the editor shows.
#[derive(Clone, Copy, Debug)]
pub struct CommandBlockInput<'a> {
    pub command: &'a str,
    pub hover_note: &'a str,
    pub tick_delay: i32,
    pub last_output: &'a str,
    pub mode: CommandBlockMode,
    pub conditional: bool,
    pub needs_redstone: bool,
    pub track_output: bool,
    pub execute_on_first_tick: bool,
    pub performed: PerformedModes,
    /// The block tracked its output when the screen opened, and still does.
    pub shows_previous: bool,
    pub maximized: bool,
    pub open: OpenDropdowns,
}

const fn condition_label(conditional: bool) -> &'static str {
    if conditional {
        "commandBlockScreen.condition.conditional"
    } else {
        "commandBlockScreen.condition.unconditional"
    }
}

const fn redstone_label(needs_redstone: bool) -> &'static str {
    if needs_redstone {
        "commandBlockScreen.redstone.needs_redstone"
    } else {
        "commandBlockScreen.redstone.always_on"
    }
}

fn color(rgba: [f64; 4]) -> Scalar {
    Scalar::Json(Value::from(rgba.to_vec()))
}

/// The screen controller's bindings for `input`, with its language keys through `translate`.
fn command_block_data(
    input: &CommandBlockInput<'_>,
    translate: &dyn Fn(&str) -> String,
) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    let text = |value: &str| Scalar::Text(value.to_owned());
    let label = |key: &str| Scalar::Text(translate(key));
    let previous = |key: &str| {
        Scalar::Text(if input.shows_previous {
            translate(key)
        } else {
            String::new()
        })
    };
    let output = match (input.shows_previous, input.last_output) {
        (false, _) => "",
        (true, "") => "-",
        (true, output) => output,
    };
    let performed = input.performed;
    let values = [
        ("#command_text_edit", text(input.command)),
        ("#command_hover_note", text(input.hover_note)),
        (
            "#command_tick_delay",
            Scalar::Text(input.tick_delay.to_string()),
        ),
        ("#command_output_text", text(output)),
        ("#char_limit_warning_visible", Scalar::Bool(false)),
        ("#maximized_input_visible", Scalar::Bool(input.maximized)),
        (
            "#minimize_button_visible_binding_name",
            Scalar::Bool(input.maximized),
        ),
        (
            "#close_button_visible_binding_name",
            Scalar::Bool(!input.maximized),
        ),
        (
            "#Xout_button_visible_binding_name",
            Scalar::Bool(!input.maximized),
        ),
        ("#divider_visible", Scalar::Bool(!input.maximized)),
        ("#track_output_binding", Scalar::Bool(input.track_output)),
        (
            "#execute_on_first_tick",
            Scalar::Bool(input.execute_on_first_tick),
        ),
        (
            "#execute_on_first_tick_enabled",
            Scalar::Bool(input.mode == CommandBlockMode::Repeat),
        ),
        (
            "#previous_block_type_text",
            previous(performed.mode.label()),
        ),
        (
            "#previous_condition_mode_text",
            previous(condition_label(performed.conditional)),
        ),
        (
            "#previous_redstone_mode_text",
            previous(redstone_label(performed.needs_redstone)),
        ),
        (
            "#previous_block_type_text_color",
            color(performed.mode.previous_color()),
        ),
        (
            "#block_type_dropdown_label_color_binding",
            color(input.mode.dropdown_color()),
        ),
        ("#block_type_icon_texture", text(input.mode.icon())),
        (
            "#block_type_dropdown_toggle_label",
            label(input.mode.label()),
        ),
        (
            "#condition_dropdown_toggle_label",
            label(condition_label(input.conditional)),
        ),
        (
            "#redstone_dropdown_toggle_label",
            label(redstone_label(input.needs_redstone)),
        ),
        (
            "#block_type_dropdown_enabled",
            Scalar::Bool(!input.maximized),
        ),
        (
            "#condition_dropdown_enabled",
            Scalar::Bool(!input.maximized),
        ),
        ("#redstone_dropdown_enabled", Scalar::Bool(!input.maximized)),
        ("#block_type_dropdown", Scalar::Bool(input.open.block_type)),
        ("#condition_dropdown", Scalar::Bool(input.open.condition)),
        ("#redstone_dropdown", Scalar::Bool(input.open.redstone)),
        (
            "#command_impulse_mode",
            Scalar::Bool(input.mode == CommandBlockMode::Impulse),
        ),
        (
            "#command_repeat_mode",
            Scalar::Bool(input.mode == CommandBlockMode::Repeat),
        ),
        (
            "#command_chain_mode",
            Scalar::Bool(input.mode == CommandBlockMode::Chain),
        ),
        ("#command_conditional_mode", Scalar::Bool(input.conditional)),
        (
            "#command_unconditional_mode",
            Scalar::Bool(!input.conditional),
        ),
        (
            "#command_needs_redstone_mode",
            Scalar::Bool(input.needs_redstone),
        ),
        (
            "#command_always_on_mode",
            Scalar::Bool(!input.needs_redstone),
        ),
    ];
    for (name, value) in values {
        data.set_global(name, value);
    }
    data
}

/// Paints the editor under `view`, whose components hold the edit boxes' live text.
pub fn paint(
    engine: &FormEngine,
    input: &CommandBlockInput<'_>,
    view: &ViewState,
    clocks: &BTreeMap<String, f64>,
    frame: &mut PaintFrame<'_>,
) -> Result<Option<EngineFrame>, PaintError> {
    let translate = frame.translate;
    let data = command_block_data(input, &|key| translated(translate, key, key));
    let art = ScreenArt {
        clocks: Some(clocks),
        ..ScreenArt::default()
    };
    engine.render_screen(
        COMMAND_BLOCK_SCREEN,
        &data,
        &crate::menu_screens::retail_context(),
        view,
        art,
        frame,
    )
}
