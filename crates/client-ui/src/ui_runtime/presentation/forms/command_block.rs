//! Draws the open command block editor and keeps its frame for input.

use ui::UiNode;

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::frame::EngineFrame;
use ui_engine::text_metrics::TextMetrics;

impl UiPresentationRuntime {
    /// The editor's last drawn frame, which its input is routed against.
    pub fn command_block_frame(&self) -> Option<&EngineFrame> {
        self.projections.command_block.as_ref()
    }

    pub(in crate::ui_runtime::presentation) fn append_command_block(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<(), UiPresentationError> {
        let editor = runtime.command_block();
        let (Some(edit), Some(renderer)) = (editor.active(), self.engine.as_deref()) else {
            return Ok(());
        };
        let translate = |key: &str| runtime.translation(key);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        self.projections.command_block = ui_screens::command_block_screen::paint(
            renderer,
            &edit.input(),
            &editor.engine().view,
            &self.scene_clock,
            &mut paint_frame,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use json_ui::{HitKind, HitRegion};
    use semantic_input::KeyboardKey;
    use ui::{DpiScale, UiNode, UiPoint};
    use ui_engine::text_metrics::TextMetrics;
    use ui_screens::command_block_screen::CommandBlockMode;
    use world::{NbtCompound, NbtValue};

    use crate::ui_runtime::UiRuntime;
    use crate::ui_runtime::command_block::CommandBlockEdit;
    use crate::ui_runtime::forms::engine_input::{EngineInput, PointerButtons};
    use crate::ui_runtime::presentation::UiPresentationRuntime;
    use crate::ui_runtime::presentation::forms::pack_harness;

    const SIZE: [u32; 2] = [1280, 720];

    /// A runtime showing the editor over a block holding a command.
    fn open_editor() -> UiRuntime {
        let mut actor = NbtCompound::default();
        actor.insert("Command", NbtValue::String("say hi".into()));
        actor.insert("TickDelay", NbtValue::Int(7));
        actor.insert("TrackOutput", NbtValue::Byte(1));
        let edit = CommandBlockEdit::load(
            [2, -60, 0],
            CommandBlockMode::Impulse,
            Some(false),
            &actor,
            |_| None,
        );
        let mut runtime = UiRuntime::new(1);
        runtime.command_block_mut().open(edit);
        runtime
    }

    /// Paints the editor as a frame build does.
    fn build(presentation: &mut UiPresentationRuntime, runtime: &UiRuntime) {
        let mut nodes: Vec<UiNode> = Vec::new();
        let mut next = 0;
        let metrics =
            TextMetrics::for_viewport(SIZE, DpiScale::new(1.0).unwrap(), Default::default());
        presentation
            .append_command_block(
                runtime,
                &mut nodes,
                &mut next,
                metrics,
                SIZE[0] as f32,
                SIZE[1] as f32,
            )
            .unwrap();
    }

    fn toggle<'a>(presentation: &'a UiPresentationRuntime, name: &str) -> Option<&'a HitRegion> {
        presentation
            .command_block_frame()?
            .hits
            .iter()
            .find(|region| region.control_name.as_deref() == Some(name) && region.enabled)
    }

    /// One press and release at the center of `region`.
    fn click(runtime: &mut UiRuntime, presentation: &UiPresentationRuntime, region: &HitRegion) {
        let frame = presentation.command_block_frame().unwrap().clone();
        let cursor = UiPoint::new(
            frame.origin[0] + (region.rect.x + region.rect.w / 2.0) as f32 * frame.scale,
            frame.origin[1] + (region.rect.y + region.rect.h / 2.0) as f32 * frame.scale,
        )
        .unwrap();
        for (down, held) in [(true, true), (false, false)] {
            let keys: [KeyboardKey; 0] = [];
            runtime.command_block_mut().drive(
                &frame,
                EngineInput {
                    cursor: Some(cursor),
                    keys: &keys,
                    pointer: PointerButtons {
                        pressed: down,
                        released: !down,
                        held,
                    },
                    touch: false,
                    pointer_edges: vec![down],
                    wheel: Vec::new(),
                    typed: Vec::new(),
                    menu_cancel: None,
                    now: 1.0,
                    animator: None,
                },
            );
        }
    }

    fn edit_box_names(presentation: &UiPresentationRuntime) -> Vec<String> {
        let mut names: Vec<_> = presentation
            .command_block_frame()
            .unwrap()
            .hits
            .iter()
            .filter(|region| region.kind == HitKind::EditBox)
            .filter_map(|region| region.widget.edit.as_ref()?.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn carrier_command_block_screen_draws_and_answers_its_controls() {
        let Some(mut presentation) = pack_harness::engine_presentation() else {
            eprintln!(
                "skipping carrier_command_block_screen_draws_and_answers_its_controls: missing UI carrier; run `make assets`"
            );
            return;
        };
        let mut runtime = open_editor();
        build(&mut presentation, &runtime);
        assert!(presentation.command_block_frame().is_some());
        let boxes = edit_box_names(&presentation);
        for name in [
            "command_block.command_text",
            "command_block.hover_note_textbox",
            "command_block.tick_delay_textbox",
        ] {
            assert!(
                boxes.iter().any(|found| found == name),
                "{name} in {boxes:?}"
            );
        }
        let edit = |runtime: &UiRuntime| runtime.command_block().active().cloned().unwrap();

        // The block type dropdown expands, offers its radio entries, and collapses on a choice.
        let dropdown = toggle(&presentation, "block_type_dropdown")
            .expect("the block type dropdown is drawn")
            .clone();
        click(&mut runtime, &presentation, &dropdown);
        assert!(edit(&runtime).input().open.block_type);
        build(&mut presentation, &runtime);
        let repeat = toggle(&presentation, "#command_repeat_mode")
            .expect("the open dropdown offers repeat")
            .clone();
        click(&mut runtime, &presentation, &repeat);
        let chosen = edit(&runtime);
        assert_eq!(chosen.input().mode, CommandBlockMode::Repeat);
        assert!(chosen.input().execute_on_first_tick);
        assert!(!chosen.input().open.block_type);

        // The screen's close button asks it to close.
        build(&mut presentation, &runtime);
        let close = presentation
            .command_block_frame()
            .unwrap()
            .hits
            .iter()
            .find(|region| region.pressed.as_deref() == Some("button.menu_exit"))
            .expect("the close button is drawn")
            .clone();
        click(&mut runtime, &presentation, &close);
        assert!(runtime.command_block().exit_requested());
    }
}
