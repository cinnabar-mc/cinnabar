//! Preserve a retained message's screen position when the history factory changes.

use super::*;
use json_ui::{Draw, DrawNode, FormRender, LayoutEnv};

pub(super) struct Anchor {
    key: String,
    y: f64,
}

/// Remember message labels nearest the viewport top, including any unpainted wheel delta.
pub(super) fn anchors(
    nodes: &[DrawNode],
    scroll: Option<&(String, ScrollMetrics)>,
    from_bottom: f64,
) -> Vec<Anchor> {
    let Some((key, metrics)) = scroll.filter(|_| from_bottom > 0.0) else {
        return Vec::new();
    };
    let delta = metrics.offset - (metrics.max_offset() - from_bottom).max(0.0);
    let mut anchors: Vec<_> = nodes
        .iter()
        .filter(|node| {
            node.key.starts_with(key)
                && node.key.split('/').any(|part| {
                    part.strip_prefix(MESSAGE_ROW_PREFIX)
                        .is_some_and(|id| id.parse::<u64>().is_ok())
                })
                && matches!(&node.draw, Draw::Text { text, .. } if !text.is_empty())
        })
        .map(|node| Anchor {
            key: node.key.clone(),
            y: node.dest.y + delta - metrics.viewport_top,
        })
        .collect();
    anchors.sort_by(|left, right| left.y.abs().total_cmp(&right.y.abs()));
    anchors
}

/// Measure updated rows, then restore the nearest surviving label before emitting pixels or hits.
#[allow(clippy::too_many_arguments)]
pub(super) fn render<'a>(
    screen: &'a mut CachedScreen,
    reference: &str,
    catalog: &Arc<json_ui::Catalog>,
    context: &json_ui::Context,
    data: Arc<DataSource>,
    at: ([f64; 2], f32, [usize; 3]),
    env: &LayoutEnv<'_>,
    view: &ViewState,
    anchors: &[Anchor],
) -> Option<&'a FormRender> {
    if anchors.is_empty() {
        return screen.render_shared_with(reference, catalog, context, data, at, env, view);
    }
    let drawn = screen.render_shared_with(
        reference,
        catalog,
        context,
        Arc::clone(&data),
        at,
        env,
        view,
    )?;
    let anchored = anchored_view(drawn, at.0, env, view, anchors);
    // Re-enter the cache so an unchanged view reuses its layout without another pass.
    screen.render_shared_with(
        reference,
        catalog,
        context,
        data,
        at,
        env,
        anchored.as_ref().unwrap_or(view),
    )
}

/// Returns `view` scrolled so the first surviving anchor keeps its viewport offset,
/// or `None` when the drawn form has no message history.
fn anchored_view(
    drawn: &FormRender,
    origin: [f64; 2],
    env: &LayoutEnv<'_>,
    view: &ViewState,
    anchors: &[Anchor],
) -> Option<ViewState> {
    let (key, metrics) = drawn
        .report
        .scrolls
        .iter()
        .find(|(key, _)| key.contains(MESSAGES_VIEW))?;
    let offset = anchors
        .iter()
        .find_map(|anchor| {
            let node = drawn.nodes.iter().find(|node| node.key == anchor.key)?;
            Some(metrics.offset + node.dest.y - metrics.viewport_top - anchor.y)
        })
        .or_else(|| {
            // A burst can move every anchor outside the emitted clip even though
            // the rows survive. Measure the bound tree without viewport culling.
            let (laid, _) = json_ui::layout_with(&drawn.bound, origin, env, view);
            anchors.iter().find_map(|anchor| {
                let y = label_y(&laid, &anchor.key)?;
                Some(metrics.offset + y - metrics.viewport_top - anchor.y)
            })
        })
        .unwrap_or(0.0)
        .clamp(0.0, metrics.max_offset());
    let mut anchored = view.clone();
    anchored.scroll.insert(key.clone(), offset);
    anchored
        .scroll_state
        .insert(key.clone(), json_ui::ScrollRetained::default());
    Some(anchored)
}

/// Find a retained label even when its current position lies outside the viewport.
fn label_y(node: &json_ui::LaidOut<'_>, key: &str) -> Option<f64> {
    if node.key == key && node.visible {
        return Some(node.rect.y);
    }
    node.children.iter().find_map(|child| label_y(child, key))
}
