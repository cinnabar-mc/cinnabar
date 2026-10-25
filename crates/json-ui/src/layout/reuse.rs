//! Reusing the last layout's output for subtrees whose controls and placement
//! inputs are unchanged. Each placed control records its inputs and where its
//! draws and hit regions sit relative to its parent's; a later layout that
//! reaches it with the same inputs splices that output instead of placing it.

use std::sync::Arc;

use super::measure::Memo;
use super::{LaidOut, LayoutEnv, Rect};
use crate::anim::Inherited;
use crate::emit::{DrawNode, RectOut, StateGate};
use crate::input::{HitRegion, HitScope};
use crate::state::ViewState;

#[cfg(test)]
mod tests;

/// The root's record key; it moves between layouts, so its address cannot name it.
pub(super) const ROOT: usize = 0;

/// What a control is placed with, besides the layout-wide view state.
#[derive(Clone)]
pub(super) struct Inputs {
    pub(super) key: String,
    pub(super) rect: Rect,
    pub(super) parent_clip: Rect,
    pub(super) parent_rect: Rect,
    pub(super) layer: i32,
    pub(super) shown: bool,
    pub(super) packed: bool,
    pub(super) allows: bool,
    pub(super) enabled: bool,
    /// Whether every ancestor shows, so the subtree emits.
    pub(super) emitting: bool,
    pub(super) inherited: Inherited,
    pub(super) hits: Arc<HitScope>,
    /// Whether a culling layout drops this subtree's scroll content outside the viewport.
    pub(super) culls: bool,
    /// The enclosing scroll views' whole solved state ([`super::scroll::ScrollFrame::applied`]).
    pub(super) scroll: u64,
}

impl Inputs {
    fn same(&self, other: &Inputs) -> bool {
        self.key == other.key
            && self.rect == other.rect
            && self.parent_clip == other.parent_clip
            && self.parent_rect == other.parent_rect
            && self.layer == other.layer
            && self.shown == other.shown
            && self.packed == other.packed
            && self.allows == other.allows
            && self.enabled == other.enabled
            && self.emitting == other.emitting
            && self.culls == other.culls
            && self.scroll == other.scroll
            && self.inherited.same(&other.inherited)
            && (Arc::ptr_eq(&self.hits, &other.hits) || self.hits == other.hits)
    }
}

/// One control's last placement.
struct Record {
    inputs: Inputs,
    /// Nothing in the subtree reads context outside its inputs.
    cacheable: bool,
    /// The layout that last placed its children.
    placed: u64,
    /// The layout that last emitted it.
    seen: u64,
    /// Draws and hit regions, as `(offset from the parent's first, count)`.
    draws: (usize, usize),
    hits: (usize, usize),
    /// The subtree's last `button.menu_cancel` global target.
    cancel: Option<String>,
    /// The subtree's first visible `root_panel`.
    root_panel: Option<RectOut>,
}

/// Where a control's output sat in the last layout, and when it was placed.
#[derive(Clone, Copy)]
pub(super) struct Old {
    draws: usize,
    hits: usize,
    placed: u64,
}

/// A subtree spliced from the last layout.
#[derive(Clone, Debug)]
pub(crate) struct Reused {
    draws: std::ops::Range<usize>,
    hits: std::ops::Range<usize>,
    cancel: Option<String>,
    root_panel: Option<RectOut>,
}

/// The last layout's records and output, kept by a [`super::MeasureCache`].
#[derive(Default)]
pub(super) struct Placements {
    records: Memo<usize, Record>,
    generation: u64,
    /// The view state, culling and geometry identity the records were made under.
    view: Option<(ViewState, bool, crate::LayoutCacheKey)>,
    /// Controls the last layout placed rather than spliced.
    placed: usize,
    /// Controls that follow a scroll view's state ([`super::scroll::pin`]), once found for
    /// the current tree.
    pinned: Option<Memo<usize, ()>>,
    /// Each scroll view's role controls when a layout last opened it, by record key.
    scroll_roles: Memo<usize, [Option<usize>; 5]>,
    /// The last output's draws and hit regions, shared with it.
    draws: Retained<DrawNode>,
    hits: Retained<HitRegion>,
}

/// A layout's draws or hit regions in output order, shared with that output,
/// and each one's output index in document order.
struct Retained<T> {
    output: Arc<[T]>,
    positions: Vec<usize>,
}

impl<T> Default for Retained<T> {
    fn default() -> Self {
        Self {
            output: Arc::from([]),
            positions: Vec::new(),
        }
    }
}

impl<T: Clone> Retained<T> {
    /// Moves the items at document indices `range` onto `out`, leaving `vacant` in
    /// their place. The output is copied first only while a caller still holds it.
    fn take(&mut self, range: std::ops::Range<usize>, out: &mut Vec<T>, vacant: fn() -> T) {
        if range.is_empty() {
            return;
        }
        let items = Arc::make_mut(&mut self.output);
        out.extend(
            self.positions[range]
                .iter()
                .map(|&at| std::mem::replace(&mut items[at], vacant())),
        );
    }

    /// Keeps `items`, given in document order, sorted stably by `key` into the
    /// output order, which it returns.
    fn keep<K: Ord>(items: Vec<T>, key: impl Fn(&T) -> K) -> (Self, Arc<[T]>) {
        let mut indexed: Vec<(usize, T)> = items.into_iter().enumerate().collect();
        indexed.sort_by_key(|(_, item)| key(item));
        let mut positions = vec![0; indexed.len()];
        let output: Arc<[T]> = indexed
            .into_iter()
            .enumerate()
            .map(|(at, (document, item))| {
                positions[document] = at;
                item
            })
            .collect();
        let kept = Self {
            output: Arc::clone(&output),
            positions,
        };
        (kept, output)
    }
}

/// The draw a spliced one leaves behind in the retained output.
fn vacant_draw() -> DrawNode {
    let none = RectOut {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    DrawNode {
        name: String::new(),
        key: String::new(),
        dest: none,
        clip: none,
        layer: 0,
        alpha: 0.0,
        paint_region: None,
        anim: None,
        draw: crate::emit::Draw::Solid { color: [0; 4] },
        gates: Vec::new(),
    }
}

/// The hit region a spliced one leaves behind in the retained output.
fn vacant_hit() -> HitRegion {
    let none = RectOut {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    HitRegion {
        key: String::new(),
        name: String::new(),
        kind: crate::input::HitKind::Button,
        rect: none,
        clip: none,
        layer: 0,
        order: 0,
        pressed: None,
        control_name: None,
        collection_index: None,
        collection: None,
        enabled: false,
        checked: None,
        max_length: None,
        group_index: None,
        renderer: None,
        drag_axes: [false; 2],
        sound: None,
        focus: None,
        widget: crate::component::Widget::default(),
        collections: Vec::new(),
        input: crate::input::InputComponent::default(),
        modal_root: None,
        transform: None,
    }
}

impl Placements {
    /// Starts a layout; changes to the view, screen or coverage pixel invalidate every record,
    /// as does a layout that will not `record` its own.
    pub(super) fn begin(
        &mut self,
        view: &ViewState,
        cull: bool,
        layout: crate::LayoutCacheKey,
        record: bool,
    ) {
        if !record
            || self
                .view
                .as_ref()
                .is_none_or(|(last, culled, last_layout)| {
                    last != view || *culled != cull || *last_layout != layout
                })
        {
            *self = Self {
                view: Some((view.clone(), cull, layout)),
                generation: self.generation,
                ..Self::default()
            };
        }
        self.generation += 1;
        self.placed = 0;
    }

    /// Controls the last layout placed rather than spliced.
    pub(super) fn placed_count(&self) -> usize {
        self.placed
    }

    /// Finds the controls under `root` that follow a scroll view's state, unless known for
    /// this tree. Needs the live role memo of a layout of `root`.
    pub(super) fn pin(&mut self, root: &super::ResolvedControl) {
        if self.pinned.is_none() {
            let mut pinned = Memo::default();
            super::scroll::pin(root, &mut pinned);
            self.pinned = Some(pinned);
        }
    }

    /// The tree changed: its pinned controls must be found again.
    pub(super) fn unpin(&mut self) {
        self.pinned = None;
    }

    /// Whether `control` follows a scroll view's state, so it is placed every layout.
    pub(super) fn pinned(&self, control: usize) -> bool {
        self.pinned
            .as_ref()
            .is_none_or(|pinned| pinned.contains_key(&control))
    }

    /// Notes the role controls of the scroll view keyed `view`; whether they differ from
    /// the last layout's, or the view is new to it. Records made before a role moved can
    /// enclose a control that now follows the view's state, so the caller splices nothing more.
    pub(super) fn roles_moved(&mut self, view: usize, roles: [Option<usize>; 5]) -> bool {
        self.scroll_roles.insert(view, roles) != Some(roles)
    }

    /// A changed control can no longer be spliced, though its children may; a
    /// removed one's record goes.
    pub(super) fn forget(&mut self, address: usize, removed: bool) {
        if removed {
            self.records.remove(&address);
            self.scroll_roles.remove(&address);
        } else if let Some(record) = self.records.get_mut(&address) {
            record.cacheable = false;
        }
    }

    /// Where `address` sat in the last layout, given where its parent did.
    pub(super) fn old(&self, address: usize, parent: Option<Old>) -> Option<Old> {
        let record = self.records.get(&address)?;
        let (draws, hits) = match parent {
            Some(parent) if record.seen == parent.placed => {
                (parent.draws + record.draws.0, parent.hits + record.hits.0)
            }
            None if address == ROOT && record.seen + 1 == self.generation => (0, 0),
            _ => return None,
        };
        Some(Old {
            draws,
            hits,
            placed: record.placed,
        })
    }

    /// The last output of `address` at `old`, when its inputs still match.
    pub(super) fn reuse(&self, address: usize, old: Old, inputs: &Inputs) -> Option<Reused> {
        let record = self.records.get(&address)?;
        (record.cacheable && record.inputs.same(inputs)).then(|| Reused {
            draws: old.draws..old.draws + record.draws.1,
            hits: old.hits..old.hits + record.hits.1,
            cancel: record.cancel.clone(),
            root_panel: record.root_panel,
        })
    }

    /// Record a control this layout placed; the output walk fills its spans.
    pub(super) fn placed(&mut self, address: usize, inputs: Inputs, cacheable: bool) {
        self.placed += 1;
        let generation = self.generation;
        let record = Record {
            inputs,
            cacheable,
            placed: generation,
            seen: 0,
            draws: (0, 0),
            hits: (0, 0),
            cancel: None,
            root_panel: None,
        };
        self.records.insert(address, record);
    }
}

/// The output of a layout over a [`Placements`].
pub(crate) struct Output {
    pub(crate) nodes: Arc<[DrawNode]>,
    pub(crate) hits: Arc<[HitRegion]>,
    pub(crate) cancel: Option<String>,
    pub(crate) root_panel: Option<RectOut>,
}

/// Emit `root`'s draws, hit regions, cancel target and `root_panel`, splicing
/// reused subtrees and recording every emitted control's spans. `gated` emits
/// as [`crate::emit_gated`] does.
pub(super) fn output(
    root: &LaidOut,
    env: &LayoutEnv,
    placements: &mut Placements,
    gated: bool,
) -> Output {
    let mut walk = Walk {
        env,
        previous_draws: std::mem::take(&mut placements.draws),
        previous_hits: std::mem::take(&mut placements.hits),
        placements,
        draws: Vec::new(),
        hits: Vec::new(),
        order: 0,
        gating: gated.then(Gating::default),
    };
    let (cancel, root_panel) = walk.node(root, ROOT, (0, 0), Some(&HitScope::default()), true);
    let Walk {
        draws,
        hits,
        placements,
        ..
    } = walk;
    let nodes;
    (placements.draws, nodes) = Retained::keep(draws, |node| node.layer);
    let sorted_hits;
    (placements.hits, sorted_hits) = Retained::keep(hits, |region| (region.layer, region.order));
    Output {
        nodes,
        hits: sorted_hits,
        cancel,
        root_panel,
    }
}

struct Walk<'p, 'e, 'x> {
    env: &'e LayoutEnv<'x>,
    placements: &'p mut Placements,
    previous_draws: Retained<DrawNode>,
    previous_hits: Retained<HitRegion>,
    draws: Vec<DrawNode>,
    hits: Vec<HitRegion>,
    order: usize,
    /// The gated emit's state, when it emits gated.
    gating: Option<Gating>,
}

/// The state children enclosing the walk: the gates they show under, and the
/// targets enclosing widgets show or hide.
#[derive(Default)]
struct Gating {
    gates: Vec<StateGate>,
    pending: Vec<(usize, StateGate)>,
}

impl Walk<'_, '_, '_> {
    /// Emit `node` (when `scope` is set, its ancestors all show; when `drawing`, a
    /// gated walk's ancestors all draw), returning the subtree's cancel target and
    /// `root_panel`.
    fn node(
        &mut self,
        node: &LaidOut,
        address: usize,
        parent: (usize, usize),
        scope: Option<&HitScope>,
        drawing: bool,
    ) -> (Option<String>, Option<RectOut>) {
        let start = (self.draws.len(), self.hits.len());
        let found = match &node.reused {
            Some(reused) => self.splice(reused),
            None if self.gating.is_some() => self.place_gated(node, scope, drawing),
            None => self.place(node, scope),
        };
        if let Some(record) = self.placements.records.get_mut(&address) {
            record.seen = self.placements.generation;
            record.draws = (start.0 - parent.0, self.draws.len() - start.0);
            record.hits = (start.1 - parent.1, self.hits.len() - start.1);
            record.cancel.clone_from(&found.0);
            record.root_panel = found.1;
        }
        found
    }

    fn place(
        &mut self,
        node: &LaidOut,
        scope: Option<&HitScope>,
    ) -> (Option<String>, Option<RectOut>) {
        let start = (self.draws.len(), self.hits.len());
        let shows = scope.filter(|_| node.visible);
        let mut cancel = None;
        let inner = shows.map(|scope| scope.inner(node.control, &node.key, node.rect));
        if let Some((inner, opened)) = &inner {
            let mut drawn = Vec::new();
            let mut order = 0;
            crate::emit::emit_own(node, self.env, &mut drawn, &mut order);
            self.draws
                .extend(drawn.into_iter().map(|(_, _, node)| node));
            self.hits
                .extend(crate::input::region(node, inner, *opened, &mut self.order));
            if node.control.properties.contains_key("button_mappings") {
                cancel =
                    crate::input::InputComponent::global_target(node.control, "button.menu_cancel");
            }
        }
        let mut root_panel =
            (node.control.name == "root_panel" && node.visible).then(|| node.rect.into());
        for child in &node.children {
            let address = std::ptr::from_ref(child.control).addr();
            let (child_cancel, child_panel) = self.node(
                child,
                address,
                start,
                inner.as_ref().map(|(inner, _)| &**inner),
                inner.is_some(),
            );
            cancel = child_cancel.or(cancel);
            root_panel = root_panel.or(child_panel);
        }
        (cancel, root_panel)
    }

    /// [`Self::place`] as [`crate::emit_gated`] emits: a state child the neutral
    /// state hides still draws, gated, if any state shows it; hit regions stay
    /// the neutral state's. `drawing` is whether every ancestor draws.
    fn place_gated(
        &mut self,
        node: &LaidOut,
        scope: Option<&HitScope>,
        drawing: bool,
    ) -> (Option<String>, Option<RectOut>) {
        let start = (self.draws.len(), self.hits.len());
        let gating = self.gating.as_mut().expect("gated walk");
        let address = std::ptr::from_ref(node.control).addr();
        let gate = gating
            .pending
            .iter()
            .rev()
            .find(|(target, _)| *target == address)
            .map(|(_, gate)| gate.clone());
        let draws = drawing && (node.visible || gate.as_ref().is_some_and(|gate| gate.mask != 0));
        let gated = gate.is_some() && draws;
        let pending = gating.pending.len();
        if draws {
            gating.gates.extend(gate);
            gating.pending.extend(
                node.state_targets
                    .iter()
                    .flat_map(|targets| targets.iter())
                    .map(|&(target, mask)| {
                        (
                            target,
                            StateGate {
                                key: node.key.clone(),
                                mask,
                            },
                        )
                    }),
            );
            let mut drawn = Vec::new();
            let mut order = 0;
            crate::emit::emit_own(node, self.env, &mut drawn, &mut order);
            let gates = &gating.gates;
            self.draws.extend(drawn.into_iter().map(|(_, _, mut node)| {
                node.gates.clone_from(gates);
                node
            }));
        }
        let shows = scope.filter(|_| node.visible);
        let mut cancel = None;
        let inner = shows.map(|scope| scope.inner(node.control, &node.key, node.rect));
        if let Some((inner, opened)) = &inner {
            self.hits
                .extend(crate::input::region(node, inner, *opened, &mut self.order));
            if node.control.properties.contains_key("button_mappings") {
                cancel =
                    crate::input::InputComponent::global_target(node.control, "button.menu_cancel");
            }
        }
        let mut root_panel =
            (node.control.name == "root_panel" && node.visible).then(|| node.rect.into());
        for child in &node.children {
            let address = std::ptr::from_ref(child.control).addr();
            let (child_cancel, child_panel) = self.node(
                child,
                address,
                start,
                inner.as_ref().map(|(inner, _)| &**inner),
                draws,
            );
            cancel = child_cancel.or(cancel);
            root_panel = root_panel.or(child_panel);
        }
        let gating = self.gating.as_mut().expect("gated walk");
        gating.pending.truncate(pending);
        if gated {
            gating.gates.pop();
        }
        (cancel, root_panel)
    }

    fn splice(&mut self, reused: &Reused) -> (Option<String>, Option<RectOut>) {
        self.previous_draws
            .take(reused.draws.clone(), &mut self.draws, vacant_draw);
        let start = self.hits.len();
        self.previous_hits
            .take(reused.hits.clone(), &mut self.hits, vacant_hit);
        for region in &mut self.hits[start..] {
            region.order = self.order;
            self.order += 1;
        }
        (reused.cancel.clone(), reused.root_panel)
    }
}
