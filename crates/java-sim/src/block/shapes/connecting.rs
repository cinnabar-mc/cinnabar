//! Fences, walls and panes, whose boxes follow their four horizontal neighbours.

use super::{Bounds, Collector, Shape};
use crate::block::facts::{Material, entry_of};
use crate::block::id;
use crate::world::World;

/// Which of the four horizontal neighbours a block connects to.
#[derive(Clone, Copy)]
struct Links {
    north: bool,
    south: bool,
    west: bool,
    east: bool,
}

impl Links {
    /// Evaluates `connects` on the ids of the north (−Z), south (+Z), west (−X) and east (+X)
    /// neighbours of the collector's block, in that order.
    fn around(boxes: &Collector<'_>, world: &impl World, connects: impl Fn(u16) -> bool) -> Self {
        let (x, y, z) = (boxes.x, boxes.y, boxes.z);
        Self {
            north: connects(world.block(x, y, z.wrapping_sub(1)).id),
            south: connects(world.block(x, y, z.wrapping_add(1)).id),
            west: connects(world.block(x.wrapping_sub(1), y, z).id),
            east: connects(world.block(x.wrapping_add(1), y, z).id),
        }
    }

    fn none(self) -> bool {
        !self.north && !self.south && !self.west && !self.east
    }
}

/// Whether a fence or wall with id `own` connects to a neighbour with id `neighbour`: its own
/// kind, a fence gate, or any block with an opaque material that renders as a normal block,
/// except pumpkins and melons.
fn fence_connects(own: u16, neighbour: u16) -> bool {
    let entry = entry_of(neighbour);
    if neighbour == own || entry.shape == Shape::FenceGate {
        return true;
    }
    entry.facts.opaque && entry.renders_as_cube && entry.facts.material != Material::Gourd
}

/// Whether a pane with id `own` connects to a neighbour with id `neighbour`: a full block, its
/// own kind, glass, stained glass or any pane.
fn pane_connects(own: u16, neighbour: u16) -> bool {
    let entry = entry_of(neighbour);
    entry.full_block
        || neighbour == own
        || neighbour == id::GLASS
        || neighbour == id::STAINED_GLASS
        || entry.shape == Shape::Pane
}

/// Height of fence and wall collision boxes.
const BARRIER_HEIGHT: f32 = 1.5;

/// A fence: the bar along Z when it connects north or south, then the bar along X when it
/// connects west or east or has no north/south connection. The X bar is the bare post when it
/// connects neither west nor east.
pub(super) fn fence(boxes: &mut Collector<'_>, world: &impl World, own: u16) {
    let links = Links::around(boxes, world, |neighbour| fence_connects(own, neighbour));
    let (post_min, post_max): (f32, f32) = (0.375, 0.625);
    if links.north || links.south {
        let min_z = if links.north { 0.0 } else { post_min };
        let max_z = if links.south { 1.0 } else { post_max };
        boxes.local(Bounds::new(
            post_min,
            0.0,
            min_z,
            post_max,
            BARRIER_HEIGHT,
            max_z,
        ));
    }
    if links.west || links.east || (!links.north && !links.south) {
        let min_x = if links.west { 0.0 } else { post_min };
        let max_x = if links.east { 1.0 } else { post_max };
        boxes.local(Bounds::new(
            min_x,
            0.0,
            post_min,
            max_x,
            BARRIER_HEIGHT,
            post_max,
        ));
    }
}

/// A cobblestone wall: one box spanning the post and every connected arm, narrowed to the arm's
/// width when it only runs straight north–south or west–east.
pub(super) fn wall(boxes: &mut Collector<'_>, world: &impl World, own: u16) {
    let links = Links::around(boxes, world, |neighbour| fence_connects(own, neighbour));
    let (arm_min, arm_max): (f32, f32) = (0.3125, 0.6875);
    let mut min_x: f32 = if links.west { 0.0 } else { 0.25 };
    let mut max_x: f32 = if links.east { 1.0 } else { 0.75 };
    let mut min_z: f32 = if links.north { 0.0 } else { 0.25 };
    let mut max_z: f32 = if links.south { 1.0 } else { 0.75 };
    if links.north && links.south && !links.west && !links.east {
        min_x = arm_min;
        max_x = arm_max;
    } else if !links.north && !links.south && links.west && links.east {
        min_z = arm_min;
        max_z = arm_max;
    }
    boxes.local(Bounds::new(min_x, 0.0, min_z, max_x, BARRIER_HEIGHT, max_z));
}

/// A pane or iron bars: the strip along X, then the strip along Z. Each strip spans the block
/// when it connects on both of its ends or the pane connects to nothing, covers the connected
/// half when it connects on one end, and is absent otherwise.
pub(super) fn pane(boxes: &mut Collector<'_>, world: &impl World, own: u16) {
    let links = Links::around(boxes, world, |neighbour| pane_connects(own, neighbour));
    let (strip_min, strip_max): (f32, f32) = (0.4375, 0.5625);
    let isolated = links.none();
    if let Some((min_x, max_x)) = strip_span(links.west, links.east, isolated) {
        boxes.local(Bounds::new(min_x, 0.0, strip_min, max_x, 1.0, strip_max));
    }
    if let Some((min_z, max_z)) = strip_span(links.north, links.south, isolated) {
        boxes.local(Bounds::new(strip_min, 0.0, min_z, strip_max, 1.0, max_z));
    }
}

/// The extent of a pane strip along its axis given its connections at the low and high ends, or
/// `None` when the strip is absent.
fn strip_span(low: bool, high: bool, isolated: bool) -> Option<(f32, f32)> {
    match (low, high) {
        (true, true) => Some((0.0, 1.0)),
        (true, false) => Some((0.0, 0.5)),
        (false, true) => Some((0.5, 1.0)),
        (false, false) => isolated.then_some((0.0, 1.0)),
    }
}
