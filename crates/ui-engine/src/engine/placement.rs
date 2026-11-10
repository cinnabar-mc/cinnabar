//! Moves one resolved control's paint while leaving its layout and animations unchanged.

use json_ui::{Animated, DrawNode, FormRender, ResolvedControl, SiblingKeys, child_key};

/// Where a control paints instead of where it is laid out: the point of its drawn box at the
/// fractions `anchor` lies on the same fractions of the screen, moved by `offset` GUI units and
/// kept on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPlacement<'a> {
    /// The namespace and name of the definition the control instantiates.
    pub definition: [&'a str; 2],
    pub anchor: [f64; 2],
    pub offset: [f64; 2],
}

/// A selected control's full layout key and its translation in GUI units.
pub(super) struct ControlShift {
    key: String,
    offset: [f64; 2],
}

impl ControlPlacement<'_> {
    /// Finds the first matching instance and places its drawn box on whole physical pixels.
    /// Returns none when the instance is absent or draws nothing.
    pub(super) fn shift(
        &self,
        render: &FormRender,
        root: [f64; 2],
        pixels: f32,
    ) -> Option<ControlShift> {
        let key = control_key(
            &render.bound,
            child_key("", &render.bound, 0),
            self.definition,
        )?;
        let mut placed = ControlShift {
            key,
            offset: [0.; 2],
        };
        let mut edges: Option<[f64; 4]> = None;
        for node in render
            .nodes
            .iter()
            .filter(|node| placed.for_node(node).is_some())
        {
            let rect = node.dest;
            edges = Some(edges.map_or(
                [rect.x, rect.y, rect.x + rect.w, rect.y + rect.h],
                |edges| {
                    [
                        edges[0].min(rect.x),
                        edges[1].min(rect.y),
                        edges[2].max(rect.x + rect.w),
                        edges[3].max(rect.y + rect.h),
                    ]
                },
            ));
        }
        let [left, top, right, bottom] = edges?;
        let pixels = f64::from(pixels);
        let shift = |axis: usize, start: f64, end: f64| {
            let room = (root[axis] - (end - start)).max(0.);
            let origin = (room * self.anchor[axis] + self.offset[axis]).clamp(0., room);
            ((origin - start) * pixels).round() / pixels
        };
        placed.offset = [shift(0, left, right), shift(1, top, bottom)];
        Some(placed)
    }
}

impl ControlShift {
    /// Returns the shift only for the selected instance and its descendants.
    pub(super) fn for_node(&self, node: &DrawNode) -> Option<[f64; 2]> {
        (node.key == self.key
            || node
                .key
                .strip_prefix(self.key.as_str())
                .is_some_and(|tail| tail.starts_with('/')))
        .then_some(self.offset)
    }
}

/// Finds a definition's first instance using the same collection and sibling keys as layout.
fn control_key(
    control: &ResolvedControl,
    key: String,
    [namespace, name]: [&str; 2],
) -> Option<String> {
    if control
        .base
        .as_ref()
        .is_some_and(|base| base.namespace == namespace && base.name == name)
    {
        return Some(key);
    }
    let mut siblings = SiblingKeys::default();
    control.children.iter().find_map(|child| {
        control_key(
            child,
            child_key(&key, child, siblings.of(child)),
            [namespace, name],
        )
    })
}

/// Translates sampled geometry without letting its paint transform scale or rotate the shift.
pub(super) fn translate(drawn: &mut Animated, [x, y]: [f64; 2]) {
    drawn.dest.x += x;
    drawn.dest.y += y;
    drawn.clip.x += x;
    drawn.clip.y += y;
    if let Some(transform) = &mut drawn.transform {
        let [a, b, c, d] = transform.linear;
        transform.translation[0] += x - (a * x + c * y);
        transform.translation[1] += y - (b * x + d * y);
    }
}

#[cfg(test)]
mod tests;
