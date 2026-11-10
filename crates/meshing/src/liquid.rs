/// Native liquid face separation from contacting block geometry.
pub const LIQUID_FACE_INSET: f32 = 0.001;
/// Word 2 flag: native tessellation admits the opposite winding for this face.
pub const LIQUID_TWO_SIDED_BIT: u32 = 1 << 29;
/// Word 2 flag: visible top emission lowered the shared liquid corner heights.
pub const LIQUID_TOP_INSET_BIT: u32 = 1 << 30;
/// Word 2 flag selecting the opaque depth-writing liquid route.
pub const LIQUID_DEPTH_WRITE_BIT: u32 = 1 << 31;
/// First-instance flag of a transparent draw whose instances are transparent liquid refs; the
/// shared transparent terrain shader otherwise reads them as model draw refs.
pub const TRANSPARENT_WATER_DRAW_FLAG: u32 = 1 << 31;
/// First-instance flag of a water draw whose instances name liquid records directly, in record
/// order, instead of sorted ref slots.
pub const TRANSPARENT_WATER_RECORD_FLAG: u32 = 1 << 30;

/// Visual medium containing the active camera eye.
///
/// This is resolved from palette-native liquid layers. Unknown world data is
/// deliberately [`Self::Air`] so an unloaded boundary cannot flash opaque fog.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CameraMedium {
    #[default]
    Air,
    Water,
    Lava,
}

/// A bounded Bedrock liquid level. Raw values 8..=15 are falling states and
/// retain their raw effective depth while rendering at source surface height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidLevel {
    depth: u8,
    height: u8,
    falling: bool,
}

impl LiquidLevel {
    pub const FULL_HEIGHT: u8 = u8::MAX;

    #[must_use]
    pub const fn from_variant(variant: u32) -> Option<Self> {
        if variant > 15 {
            return None;
        }
        let falling = variant >= 8;
        let depth = (variant & 7) as u8;
        let height = if falling {
            227
        } else {
            (((8 - depth as u16) * 255 + 4) / 9) as u8
        };
        Some(Self {
            depth,
            height,
            falling,
        })
    }

    #[must_use]
    pub const fn depth(self) -> u8 {
        self.depth
    }
    #[must_use]
    pub const fn height(self) -> u8 {
        self.height
    }
    #[must_use]
    pub const fn is_falling(self) -> bool {
        self.falling
    }

    #[must_use]
    pub const fn effective_depth(self) -> u8 {
        if self.falling { 0 } else { self.depth }
    }
}

use assets::{
    BlockFace, BlockFlags, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT,
    MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE, NetworkIdMode, RuntimeAssets, VisualKind,
};
use world::MeshNeighbourhood;

use crate::{
    BlockClassifier, ContributorResolver, Face, PackedLiquidQuad, PackedQuadLighting,
    ResolvedContributors, SIDE,
};

/// The complete face-material set also defines liquid compatibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiquidIdentity([u32; Face::ALL.len()]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiquidCell {
    identity: LiquidIdentity,
    level: LiquidLevel,
    depth_writing: bool,
}

impl LiquidCell {
    const fn material(self, face: Face) -> u32 {
        self.identity.0[face as usize]
    }

    const fn top_material(self, flowing: bool) -> u32 {
        if flowing {
            self.material(Face::NegativeX)
        } else {
            self.material(Face::PositiveY)
        }
    }
}

/// Liquid facts of one resolved cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiquidPart {
    present: bool,
    cell: Option<LiquidCell>,
}

impl LiquidPart {
    fn resolve(assets: &RuntimeAssets, contributors: ResolvedContributors) -> Self {
        let liquid = contributors.liquid_entry();
        let cell = liquid.and_then(|entry| {
            let depth_writing = supported_liquid_material_family(assets, entry.faces)?;
            (entry.kind == VisualKind::Liquid).then_some(LiquidCell {
                identity: LiquidIdentity(entry.faces),
                level: LiquidLevel::from_variant(entry.variant)?,
                depth_writing,
            })
        });
        Self {
            present: liquid.is_some(),
            cell,
        }
    }
}

/// Primary-occluder facts of one resolved cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OcclusionPart {
    occludes: bool,
    /// Full-cell liquid barriers block height and flow samples without hiding a liquid face.
    full_cell_barrier: bool,
    /// Bit per face: the primary occludes and that face's material is opaque.
    opaque_faces: u8,
}

impl OcclusionPart {
    fn resolve(assets: &RuntimeAssets, contributors: ResolvedContributors) -> Self {
        let occluder = contributors
            .primary_entry()
            .filter(|entry| entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE));
        Self {
            occludes: occluder.is_some(),
            full_cell_barrier: contributors.primary_entry().is_some_and(|entry| {
                entry
                    .sequential_id
                    .and_then(|id| {
                        assets::facts::liquid::pinned_facts(NetworkIdMode::Sequential, id)
                    })
                    .and_then(|facts| facts.full_cell_barrier)
                    .unwrap_or_else(|| {
                        fallback_cube_barrier(assets, entry.kind, entry.model_template)
                    })
            }),
            opaque_faces: occluder.map_or(0, |entry| {
                Face::ALL
                    .into_iter()
                    .filter(|&face| material_is_opaque(assets, entry.faces[face as usize]))
                    .fold(0, |mask, face| mask | 1 << face as u8)
            }),
        }
    }

    const fn opaque(self, face: Face) -> bool {
        self.opaque_faces & (1 << face as u8) != 0
    }
}

/// Unclassified families retain the cube-based liquid barrier fallback.
fn fallback_cube_barrier(assets: &RuntimeAssets, kind: VisualKind, model_template: u32) -> bool {
    kind == VisualKind::Cube
        || (kind == VisualKind::Model
            && assets
                .model_templates()
                .get(model_template as usize)
                .is_some_and(|template| template.flags == MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE))
}

mod sampler;
use sampler::Sampler;

trait LiquidSampler {
    /// Liquid facts, or `None` when no source sub-chunk covers the cell.
    fn liquid_part(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<LiquidPart>;

    /// Occlusion facts, or `None` when no source sub-chunk covers the cell.
    fn occlusion_part(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<OcclusionPart>;

    fn liquid(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<LiquidCell> {
        self.liquid_part(neighbourhood, coordinate)?.cell
    }

    fn open(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
        contacting_faces: &[Face],
    ) -> bool {
        match self.liquid_part(neighbourhood, coordinate) {
            None => true,
            Some(liquid) if liquid.present => false,
            Some(_) => self
                .occlusion_part(neighbourhood, coordinate)
                .is_none_or(|occlusion| {
                    !(occlusion.full_cell_barrier
                        || (occlusion.occludes
                            && contacting_faces.iter().all(|&face| occlusion.opaque(face))))
                }),
        }
    }

    fn solid(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
        contacting_face: Face,
    ) -> bool {
        self.occlusion_part(neighbourhood, coordinate)
            .is_some_and(|occlusion| occlusion.opaque(contacting_face))
    }
}

struct DirectSampler<'assets> {
    classifier: BlockClassifier,
    assets: &'assets RuntimeAssets,
    mode: NetworkIdMode,
}

impl DirectSampler<'_> {
    fn contributors(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<ResolvedContributors> {
        let (sub_chunk, local) = neighbourhood.liquid_block_source(coordinate)?;
        Some(ContributorResolver::resolve_direct(
            self.classifier,
            self.assets,
            self.mode,
            sub_chunk,
            local,
        ))
    }
}

impl LiquidSampler for DirectSampler<'_> {
    fn liquid_part(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<LiquidPart> {
        self.contributors(neighbourhood, coordinate)
            .map(|contributors| LiquidPart::resolve(self.assets, contributors))
    }

    fn occlusion_part(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<OcclusionPart> {
        self.contributors(neighbourhood, coordinate)
            .map(|contributors| OcclusionPart::resolve(self.assets, contributors))
    }
}

fn material_is_opaque(assets: &RuntimeAssets, material: u32) -> bool {
    assets.material(material).flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT) == 0
}

const fn opposite_face(face: Face) -> Face {
    match face {
        Face::NegativeX => Face::PositiveX,
        Face::PositiveX => Face::NegativeX,
        Face::NegativeY => Face::PositiveY,
        Face::PositiveY => Face::NegativeY,
        Face::NegativeZ => Face::PositiveZ,
        Face::PositiveZ => Face::NegativeZ,
    }
}

fn horizontal_contacting_faces([x, z]: [i32; 2]) -> ([Face; 2], usize) {
    let mut faces = [Face::PositiveY; 2];
    let mut count = 0;
    if x < 0 {
        faces[count] = Face::PositiveX;
        count += 1;
    } else if x > 0 {
        faces[count] = Face::NegativeX;
        count += 1;
    }
    if z < 0 {
        faces[count] = Face::PositiveZ;
        count += 1;
    } else if z > 0 {
        faces[count] = Face::NegativeZ;
        count += 1;
    }
    (faces, count)
}

fn supported_liquid_material_family(
    assets: &RuntimeAssets,
    materials: [u32; Face::ALL.len()],
) -> Option<bool> {
    if materials
        .into_iter()
        .all(|material| water_material(assets, material))
    {
        Some(false)
    } else if materials
        .into_iter()
        .all(|material| depth_writing_liquid_material(assets, material))
    {
        Some(true)
    } else {
        None
    }
}

fn water_material(assets: &RuntimeAssets, material: u32) -> bool {
    let required = MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT;
    assets.material(material).flags & required == required
}

fn depth_writing_liquid_material(assets: &RuntimeAssets, material: u32) -> bool {
    assets.material(material).flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE != 0
}

/// Resolves the visual medium at a camera-eye position in one packed liquid
/// neighbourhood. Surface height uses the same corner solver as liquid mesh
/// generation, so entering fog matches the rendered water/lava boundary.
#[must_use]
pub fn sample_camera_medium(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    local_position: [f32; 3],
) -> CameraMedium {
    if !local_position.iter().all(|value| value.is_finite()) {
        return CameraMedium::Air;
    }
    let block = local_position.map(|value| value.floor() as i32);
    let mut sampler = DirectSampler {
        classifier,
        assets,
        mode,
    };
    let Some(cell) = sampler.liquid(neighbourhood, block) else {
        return CameraMedium::Air;
    };
    let heights = corner_heights(&mut sampler, neighbourhood, block, cell.identity);
    let x = local_position[0].rem_euclid(1.0);
    let z = local_position[2].rem_euclid(1.0);
    let surface = triangulated_surface_height(heights, x, z) / 255.0;
    if local_position[1].rem_euclid(1.0) >= surface {
        return CameraMedium::Air;
    }
    if cell.depth_writing {
        CameraMedium::Lava
    } else {
        CameraMedium::Water
    }
}

/// Meshes the liquid faces of the centre sub-chunk. `water_filled` lists, sorted, the network
/// values of blocks Java fills with water (seagrass, kelp, bubble columns): when not empty,
/// as while a shader pack renders the world, water draws no face toward them, as Java
/// draws none between two water-filled cells.
pub(crate) fn mesh_liquids<L: crate::lighting::LightingInputs + ?Sized>(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    lighting_inputs: &L,
    water_filled: &[u32],
) -> (Vec<PackedLiquidQuad>, Vec<PackedQuadLighting>) {
    let center = neighbourhood
        .sub_chunk([0, 0, 0])
        .expect("MeshNeighbourhood always contains its center");
    let contains_liquid = center.storages().iter().any(|storage| {
        storage
            .palette()
            .values()
            .iter()
            .copied()
            .any(|network_value| {
                if classifier.is_air(network_value) {
                    return false;
                }
                let block = assets.resolve(mode, network_value);
                block.kind() == VisualKind::Liquid
                    && supported_liquid_material_family(
                        assets,
                        BlockFace::ALL.map(|face| block.face(face).material_id()),
                    )
                    .is_some()
            })
    });
    if !contains_liquid {
        return (Vec::new(), Vec::new());
    }
    let mut sampler = Sampler::new(classifier, assets, mode, neighbourhood);
    let mut transparent_quads = Vec::new();
    let mut depth_quads = Vec::new();
    let mut push_quad = |quad: PackedLiquidQuad| {
        if quad.is_depth_writing() {
            depth_quads.push(quad);
        } else {
            transparent_quads.push(quad);
        }
    };
    for x in 0..SIDE {
        for y in 0..SIDE {
            for z in 0..SIDE {
                let block = [x as i32, y as i32, z as i32];
                let Some(cell) = sampler.liquid(neighbourhood, block) else {
                    continue;
                };
                let heights = corner_heights(&mut sampler, neighbourhood, block, cell.identity);
                let gradient = flow_gradient(&mut sampler, neighbourhood, block, cell);
                let origin = [x as u8, y as u8, z as u8];
                let above = add(block, [0, 1, 0]);
                // A roof only covers surfaces raised to its bottom boundary.
                let top_emitted = !compatible(&mut sampler, neighbourhood, above, cell.identity)
                    && (cell.depth_writing
                        || !sampler.water_filled(neighbourhood, above, water_filled))
                    && (heights != [LiquidLevel::FULL_HEIGHT; 4]
                        || !sampler.solid(neighbourhood, above, Face::NegativeY));
                if top_emitted {
                    let material = cell.top_material(gradient != [0, 0]);
                    push_quad(
                        pack(
                            origin,
                            Face::PositiveY,
                            heights,
                            material,
                            gradient,
                            cell.level,
                            cell.depth_writing,
                        )
                        .with_top_height_inset(true)
                        .with_two_sided(true),
                    );
                }
                for face in [
                    Face::NegativeX,
                    Face::PositiveX,
                    Face::NegativeZ,
                    Face::PositiveZ,
                ] {
                    let adjacent = add(block, face_offset(face));
                    let adjacent_primary_air = layer_is_air(classifier, neighbourhood, 0, adjacent);
                    if compatible(&mut sampler, neighbourhood, adjacent, cell.identity)
                        || sampler.solid(neighbourhood, adjacent, opposite_face(face))
                        || (!cell.depth_writing
                            && !layer_is_air(classifier, neighbourhood, 1, adjacent))
                        || (!cell.depth_writing
                            && sampler.water_filled(neighbourhood, adjacent, water_filled))
                    {
                        continue;
                    }
                    let side_heights = match face {
                        Face::NegativeX => [0, heights[0], heights[3], 0],
                        Face::PositiveX => [0, heights[2], heights[1], 0],
                        Face::NegativeZ => [0, heights[1], heights[0], 0],
                        Face::PositiveZ => [0, heights[3], heights[2], 0],
                        _ => unreachable!(),
                    };
                    push_quad(
                        pack(
                            origin,
                            face,
                            side_heights,
                            cell.material(face),
                            gradient,
                            cell.level,
                            cell.depth_writing,
                        )
                        .with_top_height_inset(top_emitted)
                        .with_two_sided(adjacent_primary_air),
                    );
                }
                let below = add(block, [0, -1, 0]);
                if !compatible(&mut sampler, neighbourhood, below, cell.identity)
                    && !sampler.solid(neighbourhood, below, Face::PositiveY)
                    && (cell.depth_writing || layer_is_air(classifier, neighbourhood, 1, below))
                    && (cell.depth_writing
                        || !sampler.water_filled(neighbourhood, below, water_filled))
                {
                    push_quad(pack(
                        origin,
                        Face::NegativeY,
                        [0; 4],
                        cell.material(Face::NegativeY),
                        gradient,
                        cell.level,
                        cell.depth_writing,
                    ));
                }
            }
        }
    }
    transparent_quads.reserve(depth_quads.len());
    transparent_quads.append(&mut depth_quads);
    let mut addressed = Vec::with_capacity(transparent_quads.len());
    let mut lighting = Vec::with_capacity(transparent_quads.len());
    for quad in transparent_quads {
        let index = lighting.len() as u32;
        let block = quad.origin().map(i32::from);
        lighting.push(crate::lighting::bake_liquid_quad(
            lighting_inputs,
            block,
            quad.face(),
            lighting_positions(quad.face(), quad.heights()),
        ));
        addressed.push(
            PackedLiquidQuad::try_pack(
                quad.origin(),
                quad.face(),
                quad.heights(),
                quad.material_id(),
                index,
                quad.flow_gradient(),
                quad.is_falling(),
            )
            .map(|packed| {
                packed
                    .with_depth_write(quad.is_depth_writing())
                    .with_top_height_inset(quad.has_top_height_inset())
                    .with_two_sided(quad.is_two_sided())
            })
            .expect("previously checked liquid record"),
        );
    }
    (addressed, lighting)
}

/// Extra-layer air admits classic water contacts; primary air controls reverse winding.
fn layer_is_air(
    classifier: BlockClassifier,
    neighbourhood: &MeshNeighbourhood<'_>,
    layer: usize,
    coordinate: [i32; 3],
) -> bool {
    match neighbourhood.liquid_sample(layer, coordinate) {
        world::MeshSample::Block(network_value) => classifier.is_air(network_value),
        // An absent layer or subchunk is air at an open mesh boundary.
        world::MeshSample::Open => true,
    }
}

fn pack(
    origin: [u8; 3],
    face: Face,
    heights: [u8; 4],
    material: u32,
    gradient: [i8; 2],
    level: LiquidLevel,
    depth_writing: bool,
) -> PackedLiquidQuad {
    PackedLiquidQuad::try_pack(
        origin,
        face,
        heights,
        material,
        0,
        gradient,
        level.is_falling(),
    )
    .map(|packed| packed.with_depth_write(depth_writing))
    .expect("local liquid record is bounded")
}

fn flow_gradient<S: LiquidSampler + ?Sized>(
    sampler: &mut S,
    neighbourhood: &MeshNeighbourhood<'_>,
    block: [i32; 3],
    cell: LiquidCell,
) -> [i8; 2] {
    let current = i16::from(cell.level.effective_depth());
    let mut gradient = [0_i16; 2];
    for offset in [[-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]] {
        let adjacent = add(block, offset);
        let delta = if let Some(other) = sampler.liquid(neighbourhood, adjacent) {
            (other.identity == cell.identity)
                .then(|| i16::from(other.level.effective_depth()) - current)
        } else if sampler.open(
            neighbourhood,
            adjacent,
            &horizontal_contacting_faces([offset[0], offset[2]]).0[..1],
        ) {
            sampler
                .liquid(neighbourhood, add(adjacent, [0, -1, 0]))
                .filter(|below| below.identity == cell.identity)
                .map(|below| i16::from(below.level.effective_depth()) - current + 8)
        } else {
            None
        };
        if let Some(delta) = delta {
            gradient[0] += (offset[0] as i16) * delta;
            gradient[1] += (offset[2] as i16) * delta;
        }
    }
    [gradient[0] as i8, gradient[1] as i8]
}

fn corner_heights<S: LiquidSampler + ?Sized>(
    sampler: &mut S,
    neighbourhood: &MeshNeighbourhood<'_>,
    block: [i32; 3],
    identity: LiquidIdentity,
) -> [u8; 4] {
    [
        ([0, 0], [-1, 0], [0, -1], [-1, -1]),
        ([0, 0], [1, 0], [0, -1], [1, -1]),
        ([0, 0], [1, 0], [0, 1], [1, 1]),
        ([0, 0], [-1, 0], [0, 1], [-1, 1]),
    ]
    .map(|(center, a, b, diagonal)| {
        let include_diagonal = compatible(
            sampler,
            neighbourhood,
            add(block, [a[0], 0, a[1]]),
            identity,
        ) || compatible(
            sampler,
            neighbourhood,
            add(block, [b[0], 0, b[1]]),
            identity,
        );
        let samples = [
            Some(center),
            Some(a),
            Some(b),
            include_diagonal.then_some(diagonal),
        ];
        if samples
            .iter()
            .flatten()
            .any(|[x, z]| compatible(sampler, neighbourhood, add(block, [*x, 1, *z]), identity))
        {
            return LiquidLevel::FULL_HEIGHT;
        }
        let mut total = 0_u32;
        let mut weight = 0_u32;
        for [x, z] in samples.into_iter().flatten() {
            let coordinate = add(block, [x, 0, z]);
            if let Some(cell) = sampler.liquid(neighbourhood, coordinate) {
                if cell.identity != identity {
                    continue;
                }
                let sample_weight = if cell.level.height() >= 204 { 10 } else { 1 };
                total += u32::from(cell.level.height()) * sample_weight;
                weight += sample_weight;
            } else {
                let (contacting_faces, count) = horizontal_contacting_faces([x, z]);
                if sampler.open(neighbourhood, coordinate, &contacting_faces[..count]) {
                    weight += 1;
                }
            }
        }
        (total + weight / 2)
            .checked_div(weight)
            .map_or(0, |level| level as u8)
    })
}

fn compatible<S: LiquidSampler + ?Sized>(
    sampler: &mut S,
    neighbourhood: &MeshNeighbourhood<'_>,
    coordinate: [i32; 3],
    identity: LiquidIdentity,
) -> bool {
    sampler
        .liquid(neighbourhood, coordinate)
        .is_some_and(|cell| cell.identity == identity)
}

fn triangulated_surface_height(heights: [u8; 4], x: f32, z: f32) -> f32 {
    let [north_west, north_east, south_east, south_west] = heights.map(f32::from);
    if z <= x {
        north_west + x * (north_east - north_west) + z * (south_east - north_east)
    } else {
        north_west + x * (south_east - south_west) + z * (south_west - north_west)
    }
}
const fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

const fn face_offset(face: Face) -> [i32; 3] {
    match face {
        Face::NegativeX => [-1, 0, 0],
        Face::PositiveX => [1, 0, 0],
        Face::NegativeY => [0, -1, 0],
        Face::PositiveY => [0, 1, 0],
        Face::NegativeZ => [0, 0, -1],
        Face::PositiveZ => [0, 0, 1],
    }
}

fn lighting_positions(face: Face, heights: [u8; 4]) -> [[i16; 3]; 4] {
    let h = heights.map(i16::from);
    // Packed vertex order is part of the transparent-stream contract:
    // top NW/NE/SE/SW; bottom NW/SW/SE/NE;
    // -X bottom-N/top-N/top-S/bottom-S;
    // +X bottom-S/top-S/top-N/bottom-N;
    // -Z bottom-E/top-E/top-W/bottom-W;
    // +Z bottom-W/top-W/top-E/bottom-E.
    match face {
        Face::PositiveY => [
            [0, h[0], 0],
            [256, h[1], 0],
            [256, h[2], 256],
            [0, h[3], 256],
        ],
        Face::NegativeY => [[0, 0, 0], [0, 0, 256], [256, 0, 256], [256, 0, 0]],
        Face::NegativeX => [[0, h[0], 0], [0, h[1], 0], [0, h[2], 256], [0, h[3], 256]],
        Face::PositiveX => [
            [256, h[0], 256],
            [256, h[1], 256],
            [256, h[2], 0],
            [256, h[3], 0],
        ],
        Face::NegativeZ => [[256, h[0], 0], [256, h[1], 0], [0, h[2], 0], [0, h[3], 0]],
        Face::PositiveZ => [
            [0, h[0], 256],
            [0, h[1], 256],
            [256, h[2], 256],
            [256, h[3], 256],
        ],
    }
}

#[cfg(test)]
#[path = "liquid/sampling_tests.rs"]
mod sampling_tests;

#[cfg(test)]
mod tests {
    use super::triangulated_surface_height;

    #[test]
    fn camera_surface_matches_the_two_static_index_buffer_triangles() {
        let heights = [0, 255, 0, 0];
        assert_eq!(triangulated_surface_height(heights, 0.75, 0.25), 127.5);
        assert_eq!(triangulated_surface_height(heights, 0.25, 0.75), 0.0);
        assert_eq!(triangulated_surface_height(heights, 0.5, 0.5), 0.0);
    }
}
