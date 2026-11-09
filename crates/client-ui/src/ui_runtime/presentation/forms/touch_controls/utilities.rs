//! Touch utility menu state and placement within the safe viewport.
use super::*;

impl UiPresentationRuntime {
    /// Opens or dismisses the utility dropdown without capturing gameplay fingers.
    pub fn toggle_touch_utilities(&mut self) {
        self.screens.touch.utilities_open = !self.screens.touch.utilities_open;
    }

    /// Dismisses the dropdown when gameplay loses input or a menu choice is selected.
    pub fn close_touch_utilities(&mut self) {
        self.screens.touch.utilities_open = false;
    }

    /// Finds the largest vertical roster region clear of placed utility controls.
    pub(in super::super) fn touch_roster_region(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<std::ops::Range<f32>, UiPresentationError> {
        if !self.screens.touch.enabled {
            return Ok(0. ..content[1]);
        }
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        if self.screens.touch.base_viewport != Some((content, px, self.safe_area)) {
            let rollback = (nodes.len(), *next);
            self.append_touch_controls(runtime, nodes, next, metrics, content)?;
            nodes.truncate(rollback.0);
            *next = rollback.1;
        }
        let state = &self.screens.touch;
        let mut view = ViewState::default();
        let scales = place_controls(state, &mut view, content, px);
        let mut occupied = Vec::new();
        if let Some(frame) = &state.base {
            for hit in frame.hits.iter() {
                if !matches!(
                    control_id(hit.pressed.as_deref()),
                    Some(touch::UTILITIES | touch::CHAT | touch::MENU)
                ) {
                    continue;
                }
                let dy = view.drags.get(&hit.key).map_or(0., |shift| shift[1]);
                let top = ((hit.rect.y + dy) as f32 * frame.scale).clamp(0., content[1]);
                let height = hit.rect.h * scale_of(&scales, hit);
                let bottom =
                    ((hit.rect.y + dy + height) as f32 * frame.scale).clamp(top, content[1]);
                occupied.push(top..bottom);
            }
        }
        occupied.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut clear = 0. ..0.;
        let mut cursor: f32 = 0.;
        for band in occupied
            .into_iter()
            .chain(std::iter::once(content[1]..content[1]))
        {
            if band.start - cursor > clear.end - clear.start {
                clear = cursor..band.start;
            }
            cursor = cursor.max(band.end);
        }
        Ok(clear)
    }

    /// Requests the same debug visibility toggle as the desktop binding.
    pub fn request_touch_debug_toggle(&mut self) {
        self.screens.touch.debug_toggle_pending ^= true;
    }

    /// Consumes pending touch requests once, preserving the parity of repeated toggles.
    pub fn take_touch_debug_toggle(&mut self) -> bool {
        std::mem::take(&mut self.screens.touch.debug_toggle_pending)
    }
}

/// Applies the same saved or draft control positions to painting and roster avoidance, and
/// returns every placeable control's layout name with its scale, including unit scales, since
/// a retained screen keeps a binding's last value. A scaled control keeps its top-left corner at
/// the placed position.
pub(super) fn place_controls(
    state: &TouchControls,
    view: &mut ViewState,
    content: [f32; 2],
    px: f32,
) -> Vec<(String, f64)> {
    let saved = &state.saved;
    let mut scales = Vec::new();
    let Some(base) = &state.base else {
        return scales;
    };
    for hit in base
        .hits
        .iter()
        .filter(|hit| control_id(hit.pressed.as_deref()).is_some_and(placeable_control))
    {
        let layout_name = hit
            .pressed
            .as_deref()
            .unwrap()
            .strip_prefix("cinnabar.touch.")
            .unwrap();
        let Some(name) = mod_buttons::placement_name(&state.mod_buttons, layout_name) else {
            continue;
        };
        let (desired, scale) = if let Some(editor) = &state.editor {
            editor
                .rectangles()
                .find(|(id, _)| *id == name)
                .map_or((None, 1.), |(_, r)| {
                    (Some([r[0], r[1]]), r[2] / hit.rect.w.max(f64::EPSILON))
                })
        } else {
            let scale = f64::from(saved.scale(&name));
            let at = saved.position(&name).map(|position| {
                [
                    f64::from(position[0])
                        * (f64::from(content[0] / px) - hit.rect.w * scale).max(0.),
                    f64::from(position[1])
                        * (f64::from(content[1] / px) - hit.rect.h * scale).max(0.),
                ]
            });
            (at, scale)
        };
        let desired = desired.or_else(|| {
            (control_id(hit.pressed.as_deref()) == Some(touch::DPAD))
                .then(|| fit_inside(hit, content, px))
        });
        if let Some(at) = desired {
            view.drags
                .insert(hit.key.clone(), [at[0] - hit.rect.x, at[1] - hit.rect.y]);
        }
        scales.push((layout_name.to_owned(), scale));
    }
    scales
}

/// The uniform scale `place_controls` reported for a painted control, or `1`.
pub(super) fn scale_of(scales: &[(String, f64)], hit: &HitRegion) -> f64 {
    hit.pressed
        .as_deref()
        .and_then(|pressed| pressed.strip_prefix("cinnabar.touch."))
        .and_then(|name| scales.iter().find(|(scaled, _)| scaled == name))
        .map_or(1., |(_, scale)| *scale)
}

/// Where an unplaced control sits once moved fully on screen, as Bedrock fits its D-pad.
pub(super) fn fit_inside(hit: &HitRegion, content: [f32; 2], px: f32) -> [f64; 2] {
    let fit = |origin: f64, size: f64, side: f32| origin.min(f64::from(side / px) - size).max(0.);
    [
        fit(hit.rect.x, hit.rect.w, content[0]),
        fit(hit.rect.y, hit.rect.h, content[1]),
    ]
}

/// Places the dropdown below its movable trigger, or above it when space is short.
pub(super) fn place_dropdown(
    base: Option<&EngineFrame>,
    view: &mut ViewState,
    scales: &[(String, f64)],
    content: [f32; 2],
    px: f32,
) {
    let Some(base) = base else {
        return;
    };
    let Some(trigger) = base
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(touch::UTILITIES))
    else {
        return;
    };
    let Some(first) = base
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(touch::DEBUG_OVERLAY))
    else {
        return;
    };
    let Some(last) = base
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(touch::EMOTE))
    else {
        return;
    };
    let shift = view.drags.get(&trigger.key).copied().unwrap_or_default();
    let viewport = content.map(|size| f64::from(size / px));
    let width = first.rect.w;
    let height = last.rect.y + last.rect.h - first.rect.y;
    let x = (trigger.rect.x + shift[0]).clamp(0., (viewport[0] - width).max(0.));
    let below = trigger.rect.y + shift[1] + trigger.rect.h * scale_of(scales, trigger) + 2.;
    let y = if below + height <= viewport[1] {
        below
    } else {
        (trigger.rect.y + shift[1] - height - 2.).max(0.)
    };
    let Some(dropdown) = base
        .hits
        .iter()
        .find(|hit| hit.name == "utilities_dropdown")
    else {
        return;
    };
    // A stack's children are packed; translate the whole surface.
    view.drags
        .insert(dropdown.key.clone(), [x - first.rect.x, y - first.rect.y]);
}
