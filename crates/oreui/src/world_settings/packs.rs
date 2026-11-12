//! The resource and behaviour pack tabs of Edit world and Create world: the packs the world
//! carries (or will be created with) in server order, with controls to add, reorder and remove them.

use super::*;
use launcher::local_worlds::packs::{PackKind, WorldPack};
use launcher::menu::worlds_tab::PackRowControls;

/// Paints the pack tab `tab` and returns the content's bottom.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
    tab: Tab,
    editing: bool,
) -> Result<f32, PaintError> {
    let Some(kind) = tab.pack_kind() else {
        return Ok(area[1]);
    };
    let enabled = !local_view.busy && local_view.packs.is_some();
    let mut y = sections::row(canvas, area, area[1], |canvas, inner| {
        let b = [inner[0], inner[1], inner[2], inner[1] + canvas.r(CONTROL)];
        button(
            canvas,
            view,
            b,
            Variant::Primary,
            &format!("Add {}", kind.noun()),
            enabled.then_some(local(A::AddPack(kind))),
        )?;
        let note = match (kind, editing) {
            (PackKind::Resource, true) => {
                "Choose a .mcpack, .mcaddon or .zip file. The first pack has the highest priority."
            }
            (PackKind::Behaviour, true) => {
                "Choose a .mcpack, .mcaddon or .zip file. Packs apply the next time the world opens."
            }
            (PackKind::Resource, false) => {
                "Choose a .mcpack, .mcaddon or .zip file. Packs are added when you create the world; the first has the highest priority."
            }
            (PackKind::Behaviour, false) => {
                "Choose a .mcpack, .mcaddon or .zip file. Packs are added when you create the world."
            }
        };
        caption(canvas, note, inner, b[3] + space(canvas, 1))
    })?;
    if let Some(error) = &local_view.pack_error {
        y = sections::row(canvas, area, y, |canvas, inner| {
            Ok(inner[1]
                + canvas.text(
                    error,
                    [inner[0], inner[1]],
                    inner[2] - inner[0],
                    CAPTION,
                    DESTRUCTIVE.fill,
                    false,
                )?)
        })?;
    }
    let Some(packs) = &local_view.packs else {
        return sections::row(canvas, area, y, |canvas, inner| {
            caption(canvas, "Loading packs…", inner, inner[1])
        });
    };
    let list = packs.list(kind);
    if list.is_empty() {
        return sections::row(canvas, area, y, |canvas, inner| {
            let text = if editing {
                format!("This world has no {}s.", kind.noun())
            } else {
                format!("No {}s added.", kind.noun())
            };
            caption(canvas, &text, inner, inner[1])
        });
    }
    if editing && list.iter().any(|pack| pack.external) {
        y = sections::row(canvas, area, y, |canvas, inner| {
            caption(
                canvas,
                "Packs marked not stored are only listed by this world. Find in Minecraft copies one from the game's pack folders; Add takes a pack file instead.",
                inner,
                inner[1],
            )
        })?;
    }
    for (index, pack) in list.iter().enumerate() {
        let controls = PackRowControls::of(kind, index, list);
        y = sections::row(canvas, area, y, |canvas, inner| {
            pack_row(canvas, view, inner, pack, controls, enabled)
        })?;
    }
    Ok(y)
}

/// One pack: its name and details, with its move and remove buttons at the right.
fn pack_row(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    inner: Bounds,
    pack: &WorldPack,
    controls: PackRowControls,
    enabled: bool,
) -> Result<f32, PaintError> {
    let gap = canvas.r(0.8);
    let height = canvas.r(CONTROL);
    let mut buttons = Vec::new();
    // Only a pack the world does not hold has one, leaving the other buttons where they are.
    if controls.find.is_some() {
        buttons.push((
            "Find in Minecraft",
            Variant::Primary,
            controls.find,
            canvas.r(16.0),
        ));
    }
    buttons.extend([
        ("Up", Variant::Neutral, controls.earlier, canvas.r(5.6)),
        ("Down", Variant::Neutral, controls.later, canvas.r(7.2)),
        (
            "Remove",
            Variant::Destructive,
            Some(controls.remove),
            canvas.r(9.6),
        ),
    ]);
    let total: f32 = buttons.iter().map(|(.., width)| width + gap).sum();
    let text = [inner[0], inner[1], inner[2] - total, inner[3]];
    let mut end = label(canvas, &pack.name, text, inner[1])?;
    let detail = if pack.external {
        format!("Version {} · not stored in this world", pack.version_text())
    } else if pack.description.is_empty() {
        format!("Version {}", pack.version_text())
    } else {
        format!("Version {} · {}", pack.version_text(), pack.description)
    };
    end = end.max(caption(canvas, &detail, text, end - space(canvas, 1))?);
    let top = inner[1] + ((end - inner[1]) - height).max(0.0) * 0.5;
    let mut x = inner[2];
    for (label, variant, action, width) in buttons.into_iter().rev() {
        x -= width;
        let b = [x, top, x + width, top + height];
        x -= gap;
        // A move that has nowhere to go still takes its place, dimmed, so the buttons stay put.
        button(
            canvas,
            view,
            b,
            variant,
            label,
            action.filter(|_| enabled).map(local),
        )?;
    }
    Ok(end.max(top + height))
}
