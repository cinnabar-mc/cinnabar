//! Where vanilla lays out the HUD elements mods may read and hide, from the painted HUD.

use json_ui::{Draw, DrawNode};
use ui::mod_hud::{ElementState, ElementStates, VanillaElement};
use ui_screens::hud_renderers::{
    CURSOR_RENDERER, HUNGER_RENDERER, JAVA_MOB_EFFECTS_RENDERER, MOB_EFFECTS_RENDERER,
};

/// The vanilla control holding the scoreboard sidebar.
const SIDEBAR_CONTROL: &str = json_ui::vanilla::Control::ScoreboardSidebar.as_str();

/// Element states of the last painted HUD and crosshair screens.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct HudElements {
    pub states: ElementStates,
}

/// What vanilla draws for the HUD screen's elements this frame, before any mod hides them.
pub(in crate::ui_runtime::presentation) struct Drawn {
    /// Whether status effect icons draw.
    pub effects: bool,
    /// Whether the scoreboard sidebar draws.
    pub sidebar: bool,
    /// The hunger row's cells as `[left, top, right, bottom]` from its renderer's top left,
    /// or none while vanilla draws no hunger row.
    pub hunger: Option<[f32; 4]>,
    /// The Java effect rows' cells from their renderer's top right.
    pub java_effects: Option<[f32; 4]>,
}

impl Drawn {
    /// Captures the drawn rows of `paint`; call before hiding anything.
    pub fn of(paint: &ui_screens::hud_renderers::HudPaint, sidebar: bool) -> Self {
        Self {
            effects: !paint.effects.icons.is_empty() || !paint.java_effects.is_empty(),
            sidebar,
            hunger: cells(paint.hunger.cells(0)),
            java_effects: cells(paint.java_effects.iter().cloned()),
        }
    }
}

/// The `[left, top, right, bottom]` extent of `cells` from their renderer's origin.
fn cells(cells: impl Iterator<Item = ui_screens::hud_renderers::Cell>) -> Option<[f32; 4]> {
    cells.fold(None, |extent, cell| {
        let rect = [
            cell.at[0],
            cell.at[1],
            cell.at[0] + cell.size[0],
            cell.at[1] + cell.size[1],
        ];
        Some(extent.map_or(rect, |extent: [f32; 4]| {
            [
                extent[0].min(rect[0]),
                extent[1].min(rect[1]),
                extent[2].max(rect[2]),
                extent[3].max(rect[3]),
            ]
        }))
    })
}

impl HudElements {
    /// Records the HUD screen's elements: whether vanilla draws each this frame, ignoring
    /// hiding, and where in GUI units. Hunger covers the drawn row of shanks, effects the
    /// vanilla column's renderer and the drawn Java rows, and the sidebar its laid-out draws.
    pub fn observe_hud(&mut self, nodes: &[DrawNode], drawn: &Drawn) {
        let custom = |name: &str| {
            nodes.iter().find(
                |node| matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == name),
            )
        };
        let column = union(
            nodes,
            |node| matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == MOB_EFFECTS_RENDERER),
        );
        let rows = custom(JAVA_MOB_EFFECTS_RENDERER)
            .zip(drawn.java_effects)
            .map(|(node, extent)| {
                let right = node.dest.x + node.dest.w;
                offset(extent, [right as f32, node.dest.y as f32])
            });
        self.states[VanillaElement::StatusEffects.index()] = ElementState {
            visible: drawn.effects,
            bounds: merge(column, rows),
        };
        self.states[VanillaElement::Hunger.index()] = ElementState {
            visible: drawn.hunger.is_some(),
            bounds: custom(HUNGER_RENDERER)
                .zip(drawn.hunger)
                .map(|(node, extent)| offset(extent, [node.dest.x as f32, node.dest.y as f32])),
        };
        self.states[VanillaElement::Sidebar.index()] = ElementState {
            visible: drawn.sidebar,
            bounds: drawn
                .sidebar
                .then(|| union(nodes, |node| under(node, SIDEBAR_CONTROL)))
                .flatten(),
        };
    }

    /// Records the crosshair overlay: whether vanilla draws it and where.
    pub fn observe_crosshair(&mut self, nodes: &[DrawNode], visible: bool) {
        self.states[VanillaElement::Crosshair.index()] = ElementState {
            visible,
            bounds: union(
                nodes,
                |node| matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == CURSOR_RENDERER),
            ),
        };
    }

    /// Clears what a hidden HUD can no longer report.
    pub fn hide_all(&mut self) {
        for state in &mut self.states {
            state.visible = false;
        }
    }
}

/// `extent` moved to `origin`, as `[x, y, width, height]`.
fn offset(extent: [f32; 4], origin: [f32; 2]) -> [f32; 4] {
    [
        origin[0] + extent[0],
        origin[1] + extent[1],
        extent[2] - extent[0],
        extent[3] - extent[1],
    ]
}

/// The union of two optional `[x, y, width, height]` rectangles.
fn merge(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let left = a[0].min(b[0]);
            let top = a[1].min(b[1]);
            let right = (a[0] + a[2]).max(b[0] + b[2]);
            let bottom = (a[1] + a[3]).max(b[1] + b[3]);
            Some([left, top, right - left, bottom - top])
        }
        (a, b) => a.or(b),
    }
}

/// Whether `node` lies under a control named `control`.
fn under(node: &DrawNode, control: &str) -> bool {
    node.key
        .split('/')
        .any(|part| part.split(['[', '~']).next() == Some(control))
}

/// The union `[x, y, width, height]` of matching nodes' destinations.
fn union(nodes: &[DrawNode], matches: impl Fn(&DrawNode) -> bool) -> Option<[f32; 4]> {
    let mut edges: Option<[f64; 4]> = None;
    for node in nodes.iter().filter(|node| matches(node)) {
        let rect = [
            node.dest.x,
            node.dest.y,
            node.dest.x + node.dest.w,
            node.dest.y + node.dest.h,
        ];
        edges = Some(edges.map_or(rect, |edges| {
            [
                edges[0].min(rect[0]),
                edges[1].min(rect[1]),
                edges[2].max(rect[2]),
                edges[3].max(rect[3]),
            ]
        }));
    }
    edges.map(|[left, top, right, bottom]| {
        [left, top, right - left, bottom - top].map(|value| value as f32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A custom node drawn by `renderer` at `dest`, under the layout path `key`.
    fn node(key: &str, renderer: &str, dest: [f64; 4]) -> DrawNode {
        serde_json::from_value(serde_json::json!({
            "name":"node","key":key,
            "dest":{"x":dest[0],"y":dest[1],"w":dest[2],"h":dest[3]},
            "clip":{"x":0,"y":0,"w":1000,"h":1000},"layer":0,"alpha":1,
            "draw":{"Custom":{"renderer":renderer,"data":{}}}
        }))
        .unwrap()
    }

    #[test]
    fn elements_report_vanilla_visibility_and_the_union_of_their_draws() {
        let nodes = [
            node("root/hud/hunger", HUNGER_RENDERER, [100., 200., 81., 9.]),
            node("root/effects", MOB_EFFECTS_RENDERER, [300., 10., 24., 24.]),
            node(
                "root/effects2",
                JAVA_MOB_EFFECTS_RENDERER,
                [276., 10., 24., 24.],
            ),
            node("root/scoreboard_sidebar/row[1]", "x", [500., 100., 60., 9.]),
            node("root/scoreboard_sidebar/title", "x", [490., 90., 80., 10.]),
        ];
        let mut elements = HudElements::default();
        let drawn = Drawn {
            effects: true,
            sidebar: true,
            hunger: Some([-80., 0., 1., 9.]),
            java_effects: None,
        };
        elements.observe_hud(&nodes, &drawn);
        let state =
            |elements: &HudElements, element: VanillaElement| elements.states[element.index()];
        assert_eq!(
            state(&elements, VanillaElement::StatusEffects).bounds,
            Some([300., 10., 24., 24.])
        );
        assert!(state(&elements, VanillaElement::StatusEffects).visible);
        assert_eq!(
            state(&elements, VanillaElement::Hunger),
            ElementState {
                visible: true,
                bounds: Some([20., 200., 81., 9.])
            },
            "the drawn row of shanks extends left of the renderer's origin"
        );
        elements.observe_hud(
            &nodes,
            &Drawn {
                hunger: None,
                java_effects: Some([-18., 0., 0., 18.]),
                ..drawn
            },
        );
        assert_eq!(
            state(&elements, VanillaElement::Hunger),
            ElementState::default()
        );
        assert_eq!(
            state(&elements, VanillaElement::StatusEffects).bounds,
            Some([282., 10., 42., 24.]),
            "rows drawn left of the Java renderer's top right join the column"
        );
        assert_eq!(
            state(&elements, VanillaElement::Sidebar).bounds,
            Some([490., 90., 80., 19.])
        );
        elements.observe_crosshair(
            &[node("cross", CURSOR_RENDERER, [312., 172., 16., 16.])],
            true,
        );
        assert_eq!(
            state(&elements, VanillaElement::Crosshair).bounds,
            Some([312., 172., 16., 16.])
        );
        elements.hide_all();
        assert!(elements.states.iter().all(|state| !state.visible));
    }
}
