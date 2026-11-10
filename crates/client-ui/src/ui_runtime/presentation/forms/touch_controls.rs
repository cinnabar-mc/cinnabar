//! Gameplay touch controls drawn through JSON-UI; the painted rectangles are the touch regions.

use std::sync::Arc;

use json_ui::{Catalog, Context, DataSource, HitRegion, Scalar, ViewState, hit_test};
use semantic_input::{TouchBounds, TouchContact, TouchControlMode, TouchControlRegion, touch};
use ui::{FONT_DESIGN_PIXEL_TEXELS, UiNode, UiPoint};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{ScreenArt, screen::PaintFrame},
    frame::EngineFrame,
    text_metrics::TextMetrics,
};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;

mod editor;
mod mod_buttons;
mod utilities;

const SCREEN: &str = "cinnabar_touch.screen";
const LAYOUT: &[u8] = assets::TOUCH_LAYOUT;
/// Each control's name in the layout, which its `cinnabar.touch.<name>` button id and
/// `#touch_<name>_pressed` binding carry.
const CONTROLS: &[(&str, u16)] = &[
    ("look", touch::LOOK_SURFACE),
    ("joystick", touch::JOYSTICK),
    ("dpad", touch::DPAD),
    ("jump", touch::JUMP),
    ("sneak", touch::SNEAK),
    ("sprint", touch::SPRINT),
    ("attack", touch::ATTACK),
    ("use", touch::USE),
    ("menu", touch::MENU),
    ("chat", touch::CHAT),
    ("utilities", touch::UTILITIES),
];
const UTILITY_ACTIONS: &[(&str, u16)] = &[
    ("emote", touch::EMOTE),
    ("debug_overlay", touch::DEBUG_OVERLAY),
    ("player_list", touch::PLAYER_LIST),
    ("dismiss_utilities", touch::DISMISS_UTILITIES),
];

#[derive(Default)]
pub(in super::super) struct TouchControls {
    enabled: bool,
    mode: TouchControlMode,
    utilities_open: bool,
    debug_toggle_pending: bool,
    editor: Option<mod_ui::HudEditor>,
    /// Geometry last installed in the editor, independent of ordinary HUD paints.
    editor_viewport: Option<([f32; 2], f32, ui::SafeArea)>,
    pub(super) saved: launcher::menu::settings_options::touch_layout::TouchLayout,
    base: Option<EngineFrame>,
    base_viewport: Option<([f32; 2], f32, ui::SafeArea)>,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    /// The overlay and the HUD as the last build painted them, which touches resolve against;
    /// a build that paints neither leaves none.
    frame: Option<EngineFrame>,
    pub(super) hud: Option<EngineFrame>,
    /// Session that produced the retained HUD input geometry.
    pub(super) hud_session: Option<u64>,
    /// The stick knob's drag key and size from the last overlay paint.
    knob: Option<(String, [f64; 2])>,
    /// Held contacts, which press their controls and move the stick's knob.
    contacts: Vec<TouchContact>,
    /// Presented mod buttons in load order; a button's index is its slot.
    mod_buttons: Vec<ui::mod_touch::TouchButton>,
    /// Owner token of each presented slot.
    mod_owners: Vec<u64>,
    /// The newest set received while the editor fixed the controls.
    deferred_mod_buttons: Option<Vec<(u64, ui::mod_touch::TouchButton)>>,
    /// Completed mod button taps, with their owner tokens, until the app delivers them.
    mod_taps: Vec<(u64, String)>,
    /// Advances whenever a slot's button identity changes.
    mod_button_epoch: u64,
}

impl TouchControls {
    /// Whether gameplay should publish touch-specific HUD controls.
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    /// Retires the last build's geometry, so hidden controls cannot take touches.
    pub(super) fn begin_frame(&mut self) {
        self.frame = None;
        self.hud = None;
        self.hud_session = None;
    }
}

impl UiPresentationRuntime {
    /// Shows the gameplay touch controls; platforms without a touchscreen leave them off.
    pub fn enable_touch_controls(&mut self, enabled: bool) {
        self.screens.touch.enabled = enabled;
        if !enabled {
            self.close_touch_utilities();
        }
    }

    pub fn touch_controls_enabled(&self) -> bool {
        self.screens.touch.enabled
    }

    /// The touch control mode the overlay and world gestures follow.
    pub fn touch_control_mode(&self) -> TouchControlMode {
        self.screens.touch.mode
    }

    /// Selects the controls the touch overlay shows; measured geometry follows the new set.
    pub fn set_touch_control_mode(&mut self, mode: TouchControlMode) {
        let state = &mut self.screens.touch;
        if state.mode != mode {
            state.mode = mode;
            state.base_viewport = None;
            state.editor_viewport = None;
        }
    }

    /// Whether the touch mode replaces the crosshair with interacting where the finger is.
    pub(in super::super) fn touch_hides_crosshair(&self) -> bool {
        let state = &self.screens.touch;
        state.enabled && state.mode.interacts_by_touch()
    }

    pub fn set_touch_contacts(&mut self, contacts: Vec<TouchContact>) {
        self.screens.touch.contacts = contacts;
    }

    /// The control under a normalized `position` in a `window` of logical pixels: a hotbar slot,
    /// then an overlay control, whose bounds are the painted rectangle.
    pub fn gameplay_touch_region(
        &self,
        position: [f32; 2],
        window: [f32; 2],
    ) -> Option<TouchControlRegion> {
        let state = &self.screens.touch;
        if state.editor.is_some()
            || !state.enabled
            || window.iter().any(|size| !size.is_finite() || *size <= 0.0)
        {
            return None;
        }
        let point = UiPoint::new(position[0] * window[0], position[1] * window[1]).ok()?;
        if state.utilities_open {
            let frame = state.frame.as_ref()?;
            let hit = hit_test(&frame.hits, frame.to_virtual(point))?;
            return region_bounds(frame, hit, window, control_id(hit.pressed.as_deref())?);
        }
        if let Some(frame) = &state.hud
            && let Some(hit) = hit_test(&frame.hits, frame.to_virtual(point))
        {
            let id = if hit.pressed.as_deref() == Some("button.hotbar_inventory_button") {
                Some(touch::INVENTORY)
            } else if hit.collection.as_deref() == Some("hotbar_items") {
                hit.collection_index
                    .filter(|index| *index < 9)
                    .map(|index| touch::HOTBAR_1 + index as u16)
            } else {
                None
            };
            if let Some(id) = id {
                return region_bounds(frame, hit, window, id);
            }
        }
        let frame = state.frame.as_ref()?;
        let hit = hit_test(&frame.hits, frame.to_virtual(point))?;
        let hit_id = control_id(hit.pressed.as_deref())
            .or_else(|| (hit.name == "knob").then_some(touch::JOYSTICK))?;
        if hit_id == touch::DPAD {
            return dpad_region(frame, hit, position, window);
        }
        // The knob moves, so the stick's activation area is its frame.
        let region = if hit_id == touch::JOYSTICK {
            frame
                .hits
                .iter()
                .find(|region| control_id(region.pressed.as_deref()) == Some(hit_id))?
        } else {
            hit
        };
        region_bounds(frame, region, window, hit_id)
    }

    pub(in super::super) fn append_touch_controls(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let state = &mut self.screens.touch;
        if !state.enabled && state.editor.is_none() {
            return Ok(());
        }
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(());
        };
        let catalog = match &state.catalog {
            Some(catalog) => Arc::clone(catalog),
            None => {
                let catalog = Arc::new(
                    layout_catalog(&state.mod_buttons).expect("built-in touch layout parses"),
                );
                state.catalog = Some(Arc::clone(&catalog));
                catalog
            }
        };
        let mut view = ViewState::default();
        if let Some((key, size)) = &state.knob
            && let Some(stick) = state
                .contacts
                .iter()
                .find(|contact| contact.hit_id == Some(touch::JOYSTICK))
        {
            view.drags.insert(
                key.clone(),
                [
                    f64::from(stick.delta[0]) * size[0],
                    -f64::from(stick.delta[1]) * size[1],
                ],
            );
        }
        let mut data = DataSource::new();
        set_mode_bindings(&mut data, state.mode);
        if state.mode.uses_dpad() {
            set_dpad_bindings(&mut data, &state.contacts);
        }
        data.set_global("#touch_editor_open", Scalar::Bool(state.editor.is_some()));
        data.set_global(
            "#touch_utilities_open",
            Scalar::Bool(state.utilities_open && state.editor.is_none()),
        );
        data.set_global(
            "#touch_debug_overlay_active",
            Scalar::Bool(self.debug_lines.is_some()),
        );
        data.set_global(
            "#touch_player_list_active",
            Scalar::Bool(runtime.player_list_held()),
        );
        data.set_global("#touch_never_active", Scalar::Bool(false));
        for &(name, id) in CONTROLS {
            let held = state
                .contacts
                .iter()
                .any(|contact| contact.hit_id == Some(id));
            data.set_global(format!("#touch_{name}_pressed"), Scalar::Bool(held));
        }
        for (slot, button) in state.mod_buttons.iter().enumerate() {
            data.set_global(
                mod_buttons::label_binding(slot),
                Scalar::Text(button.label.clone()),
            );
            let id = touch::mod_button(slot);
            let held = id.is_some() && state.contacts.iter().any(|c| c.hit_id == id);
            data.set_global(format!("#touch_mod_{slot}_pressed"), Scalar::Bool(held));
        }
        let translate = |key: &str| runtime.translation(key);
        let language = runtime.text_generation();
        let mut paint = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language,
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let screen = &mut state.screen;
        let viewport_key = (content, px, self.safe_area);
        if state.base_viewport != Some(viewport_key) {
            let rollback = (paint.nodes.len(), *paint.next);
            let base_view = ViewState::default();
            let mut base_data = DataSource::new();
            base_data.set_global("#touch_utilities_open", Scalar::Bool(true));
            // The retained screen keeps the last bound scale, so measure every control at 1.
            let slots = (0..state.mod_buttons.len()).map(mod_buttons::layout_name);
            for name in CONTROLS
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .chain(slots)
            {
                base_data.set_global(editor::scale_binding(&name), Scalar::Num(1.));
            }
            set_mode_bindings(&mut base_data, state.mode);
            state.base = renderer.draw(ScreenArt::default(), &mut paint, |env, root| {
                screen.render_with(
                    SCREEN,
                    &catalog,
                    &Context::default(),
                    base_data,
                    (root, px, language),
                    env,
                    &base_view,
                )
            })?;
            paint.nodes.truncate(rollback.0);
            *paint.next = rollback.1;
            state.base_viewport = Some(viewport_key);
        }
        let scales = utilities::place_controls(state, &mut view, content, px);
        for (name, scale) in &scales {
            data.set_global(editor::scale_binding(name), Scalar::Num(*scale));
        }
        if state.utilities_open {
            view.pressed = state.base.as_ref().and_then(|base| {
                base.hits.iter().find_map(|hit| {
                    let id = control_id(hit.pressed.as_deref())?;
                    if !UTILITY_ACTIONS.iter().any(|(_, action)| *action == id) {
                        return None;
                    }
                    state
                        .contacts
                        .iter()
                        .any(|contact| contact.hit_id == Some(id))
                        .then(|| hit.key.clone())
                })
            });
        }
        utilities::place_dropdown(state.base.as_ref(), &mut view, &scales, content, px);
        let screen = &mut state.screen;
        let art = ScreenArt {
            view: Some(&view),
            ..ScreenArt::default()
        };
        state.frame = renderer.draw(art, &mut paint, |env, root| {
            screen.render_with(
                SCREEN,
                &catalog,
                &Context::default(),
                data,
                (root, px, language),
                env,
                &view,
            )
        })?;
        state.knob = state.frame.as_ref().and_then(|frame| {
            let knob = frame.hits.iter().find(|hit| hit.name == "knob")?;
            Some((knob.key.clone(), [knob.rect.w, knob.rect.h]))
        });
        Ok(())
    }
}

/// Publishes which controls the mode shows.
fn set_mode_bindings(data: &mut DataSource, mode: TouchControlMode) {
    data.set_global("#touch_joystick_shown", Scalar::Bool(!mode.uses_dpad()));
    data.set_global("#touch_dpad_shown", Scalar::Bool(mode.uses_dpad()));
    data.set_global(
        "#touch_action_buttons_shown",
        Scalar::Bool(!mode.interacts_by_touch()),
    );
}

/// Publishes the D-pad's pressed arrows from its finger's direction. The forward diagonals show
/// only while that finger moves forward.
fn set_dpad_bindings(data: &mut DataSource, contacts: &[TouchContact]) {
    let [x, y] = contacts
        .iter()
        .find(|contact| contact.hit_id == Some(touch::DPAD))
        .map_or([0.0; 2], |contact| contact.delta);
    for (name, pressed) in [
        ("up", x == 0.0 && y > 0.0),
        ("down", y < 0.0),
        ("left", x < 0.0 && y == 0.0),
        ("right", x > 0.0 && y == 0.0),
        ("up_left", x < 0.0 && y > 0.0),
        ("up_right", x > 0.0 && y > 0.0),
    ] {
        data.set_global(format!("#touch_dpad_{name}_pressed"), Scalar::Bool(pressed));
    }
    data.set_global("#touch_dpad_forward", Scalar::Bool(y > 0.0));
}

/// A press on the D-pad: its centre cell is a jump button and the rest steers.
fn dpad_region(
    frame: &EngineFrame,
    pad: &HitRegion,
    position: [f32; 2],
    window: [f32; 2],
) -> Option<TouchControlRegion> {
    let whole = region_bounds(frame, pad, window, touch::DPAD)?;
    let third = |axis: usize| (whole.bounds.max[axis] - whole.bounds.min[axis]) / 3.0;
    let centre = TouchBounds {
        min: [
            whole.bounds.min[0] + third(0),
            whole.bounds.min[1] + third(1),
        ],
        max: [
            whole.bounds.max[0] - third(0),
            whole.bounds.max[1] - third(1),
        ],
    };
    let in_centre =
        (0..2).all(|axis| centre.min[axis] <= position[axis] && position[axis] <= centre.max[axis]);
    Some(if in_centre {
        TouchControlRegion {
            hit_id: touch::JUMP,
            bounds: centre,
        }
    } else {
        whole
    })
}

/// The authored layout plus presented mod buttons, all placeable, and the editor captions.
fn layout_catalog(
    mod_buttons: &[ui::mod_touch::TouchButton],
) -> Result<Catalog, json_ui::LoadError> {
    let mut layout: serde_json::Value =
        serde_json::from_slice(LAYOUT).expect("built-in touch layout parses");
    mod_buttons::append_nodes(
        layout["screen"]["controls"]
            .as_array_mut()
            .expect("touch controls array"),
        mod_buttons,
    );
    editor::enable_placement(&mut layout);
    let controls = layout["screen"]["controls"]
        .as_array_mut()
        .expect("touch controls array");
    for (name, text, y, scale) in [
        ("editor_title", "Touch controls", 28., 1.2),
        (
            "editor_help",
            "Drag a control or a corner to resize it. Guides align edges and centers.",
            44.,
            0.65,
        ),
    ] {
        controls.push(serde_json::json!({name: {"type":"label", "text":text,
            "localize":false, "font_scale_factor":scale, "shadow":true, "text_alignment":"center",
            "anchor_from":"top_middle", "anchor_to":"top_middle", "offset":[0,y],
            "size":["100%",14], "color":[1,1,1], "visible":false,
            "bindings":[{"binding_name":"#touch_editor_open","binding_name_override":"#visible"}]}}));
    }
    let bytes = serde_json::to_vec(&layout).expect("touch layout serializes");
    Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        (
            "ui/_ui_defs.json",
            &br#"{"ui_defs":["ui/cinnabar_touch.json"]}"#[..],
        ),
        ("ui/cinnabar_touch.json", bytes.as_slice()),
    ])
    .map(Catalog::with_paint_transforms)
}

fn region_bounds(
    frame: &EngineFrame,
    region: &HitRegion,
    window: [f32; 2],
    hit_id: u16,
) -> Option<TouchControlRegion> {
    // A resized control's rect is in layout coordinates; its clip is already where it draws.
    let rect = &region.rect;
    let corners = [[rect.x, rect.y], [rect.x + rect.w, rect.y + rect.h]]
        .map(|corner| region.transform.map_or(corner, |map| map.apply(corner)));
    let left = corners[0][0].min(corners[1][0]).max(region.clip.x);
    let top = corners[0][1].min(corners[1][1]).max(region.clip.y);
    let right = corners[0][0]
        .max(corners[1][0])
        .min(region.clip.x + region.clip.w);
    let bottom = corners[0][1]
        .max(corners[1][1])
        .min(region.clip.y + region.clip.h);
    if left >= right || top >= bottom {
        return None;
    }
    let normalized = |x: f64, y: f64| {
        [
            (frame.origin[0] + x as f32 * frame.scale) / window[0],
            (frame.origin[1] + y as f32 * frame.scale) / window[1],
        ]
    };
    Some(TouchControlRegion {
        hit_id,
        bounds: TouchBounds {
            min: normalized(left, top),
            max: normalized(right, bottom),
        },
    })
}

/// Only permanent controls and mod buttons participate in saved placement and the editor.
fn placeable_control(id: u16) -> bool {
    (id != touch::LOOK_SURFACE && CONTROLS.iter().any(|&(_, control)| control == id))
        || touch::mod_button_slot(id).is_some()
}

fn control_id(pressed: Option<&str>) -> Option<u16> {
    let name = pressed?.strip_prefix("cinnabar.touch.")?;
    CONTROLS
        .iter()
        .chain(UTILITY_ACTIONS)
        .find_map(|&(control, id)| (control == name).then_some(id))
        .or_else(|| mod_buttons::slot(name).and_then(touch::mod_button))
}

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod editor_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every overlay control names a layout button in some mode, so a renamed control cannot go
    /// unreachable, and each mode paints exactly its own movement and action controls.
    #[test]
    fn every_control_is_a_layout_button() {
        let mut presentation = crate::test_support::mini_engine_presentation();
        presentation.enable_touch_controls(true);
        presentation.toggle_touch_utilities();
        let mut actual = std::collections::BTreeSet::new();
        for mode in [
            TouchControlMode::Touch,
            TouchControlMode::Crosshair,
            TouchControlMode::Classic,
        ] {
            presentation.set_touch_control_mode(mode);
            presentation
                .append_touch_controls(
                    &UiRuntime::new(1),
                    &mut Vec::new(),
                    &mut 0,
                    TextMetrics::for_viewport([854, 480], ui::DpiScale::new(1.).unwrap(), None),
                    [854., 480.],
                )
                .unwrap();
            let painted: std::collections::BTreeSet<_> = presentation
                .screens
                .touch
                .base
                .as_ref()
                .unwrap()
                .hits
                .iter()
                .filter_map(|hit| hit.pressed.as_deref())
                .filter(|action| action.starts_with("cinnabar.touch."))
                .map(|action| {
                    control_id(Some(action)).expect("painted touch action has an input id")
                })
                .collect();
            assert_eq!(painted.contains(&touch::DPAD), mode.uses_dpad(), "{mode:?}");
            assert_eq!(
                painted.contains(&touch::JOYSTICK),
                !mode.uses_dpad(),
                "{mode:?}"
            );
            assert_eq!(
                painted.contains(&touch::JUMP),
                !mode.uses_dpad(),
                "{mode:?}"
            );
            for action in [touch::ATTACK, touch::USE] {
                assert_eq!(
                    painted.contains(&action),
                    !mode.interacts_by_touch(),
                    "{mode:?}"
                );
            }
            actual.extend(painted);
        }
        let expected: std::collections::BTreeSet<_> = CONTROLS
            .iter()
            .chain(UTILITY_ACTIONS)
            .map(|&(_, id)| id)
            .collect();
        assert_eq!(expected.len(), CONTROLS.len() + UTILITY_ACTIONS.len());
        assert_eq!(actual, expected);
    }

    /// The D-pad's centre cell presses jump with its own bounds; the rest of the pad steers.
    #[test]
    fn the_dpad_centre_presses_jump() {
        let mut presentation = crate::test_support::mini_engine_presentation();
        presentation.enable_touch_controls(true);
        presentation.set_touch_control_mode(TouchControlMode::Classic);
        let size = [854.0_f32, 480.0];
        presentation
            .append_touch_controls(
                &UiRuntime::new(1),
                &mut Vec::new(),
                &mut 0,
                TextMetrics::for_viewport([854, 480], ui::DpiScale::new(1.).unwrap(), None),
                size,
            )
            .unwrap();
        let pad = presentation
            .screens
            .touch
            .frame
            .as_ref()
            .unwrap()
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("cinnabar.touch.dpad"))
            .expect("classic mode paints the pad")
            .clone();
        let frame = presentation.screens.touch.frame.as_ref().unwrap();
        let at = |x: f64, y: f64| {
            [
                (frame.origin[0] + (pad.rect.x + pad.rect.w * x) as f32 * frame.scale) / size[0],
                (frame.origin[1] + (pad.rect.y + pad.rect.h * y) as f32 * frame.scale) / size[1],
            ]
        };
        let (centre, arrow) = (at(0.5, 0.5), at(0.5, 0.1));
        let centre_hit = presentation.gameplay_touch_region(centre, size).unwrap();
        assert_eq!(centre_hit.hit_id, touch::JUMP);
        let arrow_hit = presentation.gameplay_touch_region(arrow, size).unwrap();
        assert_eq!(arrow_hit.hit_id, touch::DPAD);
        assert!(
            arrow_hit.bounds.max[0] - arrow_hit.bounds.min[0]
                > 2.0 * (centre_hit.bounds.max[0] - centre_hit.bounds.min[0])
        );
    }

    /// Hotbar cells and overlay controls from an earlier build cannot take touches once a build
    /// stops painting them, as when the HUD is hidden.
    #[test]
    fn a_build_that_paints_nothing_leaves_no_touch_regions() {
        let painted = || EngineFrame {
            hits: Arc::from([]),
            report: json_ui::LayoutReport::default(),
            cancel_target: None,
            origin: [0.0; 2],
            scale: 1.0,
            panel: None,
            edit_texts: Vec::new(),
            top: Vec::new(),
        };
        let mut controls = TouchControls {
            frame: Some(painted()),
            hud: Some(painted()),
            ..TouchControls::default()
        };
        controls.begin_frame();
        assert!(controls.frame.is_none() && controls.hud.is_none());
    }
}
