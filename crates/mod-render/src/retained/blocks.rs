//! Dense guest-owned block grids, meshed with the terrain implementation on a worker.

use super::{DrawError, ErrorCode, Geometry, error};
use assets::{NetworkIdMode, RuntimeAssets};
use mod_api::drawing::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(test)]
mod tests;

/// A state is resolved in the current session, never a different world's ID space.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BlockState {
    RuntimeId(u32),
    Named { name: String, states: String },
}

/// Independent light channels; absent values use the terrain full-bright defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockCell {
    pub palette: u32,
    pub sky_light: Option<u8>,
    pub block_light: Option<u8>,
}

/// Dense x,z,y cells with an explicit palette. Everything outside the grid is air.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockGrid {
    /// Host generation used to fence process-local completion feedback.
    pub revision: u64,
    pub size: [u32; 3],
    pub palette: Vec<BlockState>,
    pub cells: Vec<BlockCell>,
}

/// An atomic change to a cell in an existing grid palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockEdit {
    pub position: [u32; 3],
    pub cell: BlockCell,
}

/// One immutable section's original terrain streams and grid-local origin.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockSection {
    pub origin: [i32; 3],
    pub mesh: meshing::ChunkMesh,
}

/// Completed terrain output. Counts include every section, model and liquid.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockMesh {
    pub sections: Vec<Arc<BlockSection>>,
    pub triangles: usize,
    pub bytes: usize,
}

/// Retains the registry and meshing settings to which packed material addresses belong.
#[derive(Clone)]
struct MeshContext {
    assets: Arc<RuntimeAssets>,
    mode: NetworkIdMode,
    settings: meshing::MeshRenderSettings,
}
impl std::fmt::Debug for MeshContext {
    /// Asset contents stay outside resource diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MeshContext")
    }
}

/// A single worker claim and completion shared across snapshots and instances.
#[derive(Debug)]
pub struct BlockGeometry {
    pub grid: BlockGrid,
    claimed: AtomicBool,
    result: OnceLock<Result<Arc<BlockMesh>, DrawError>>,
    render_error: OnceLock<DrawError>,
    render_ready: AtomicBool,
    remote: OnceLock<Result<(), ErrorCode>>,
    context: OnceLock<MeshContext>,
    previous: Option<Arc<BlockMesh>>,
    dirty: Option<BTreeSet<[i32; 3]>>,
}

impl PartialEq for BlockGeometry {
    /// Completion changes readiness, never immutable resource identity.
    fn eq(&self, other: &Self) -> bool {
        self.grid == other.grid
    }
}

impl BlockGeometry {
    /// Pins the first session asset context; changed contexts require a new generation.
    pub fn bind_context(
        &self,
        assets: &Arc<RuntimeAssets>,
        mode: NetworkIdMode,
        settings: meshing::MeshRenderSettings,
    ) -> bool {
        let context = self.context.get_or_init(|| MeshContext {
            assets: Arc::clone(assets),
            mode,
            settings,
        });
        Arc::ptr_eq(&context.assets, assets) && context.mode == mode && context.settings == settings
    }

    /// Publishes complete native GPU arena admission for this generation.
    pub fn complete_render(&self) {
        self.render_ready.store(true, Ordering::Release);
    }

    /// Records a GPU admission failure for this immutable generation.
    pub fn fail_render(&self, error: DrawError) {
        let _ = self.render_error.set(error);
    }
    /// Reports a later GPU admission failure through the host status operation.
    pub fn render_error(&self) -> Option<&DrawError> {
        self.render_error.get()
    }

    /// Returns the local or process-reported completion without copying mesh data.
    pub fn status(&self) -> Option<Result<(), ErrorCode>> {
        self.render_error()
            .map(|error| Err(error.code))
            .or_else(|| match self.result() {
                Some(Err(error)) => Some(Err(error.code)),
                Some(Ok(_)) => self.render_ready.load(Ordering::Acquire).then_some(Ok(())),
                None => None,
            })
            .or_else(|| self.remote.get().copied())
    }

    /// Applies only feedback for the exact submitted grid generation.
    pub fn accept_status(&self, revision: u64, status: Result<(), ErrorCode>) {
        if self.grid.revision == revision {
            if let Err(code) = status {
                self.fail_render(error(code, "block renderer"));
            } else {
                let _ = self.remote.set(status);
            }
        }
    }

    /// Builds changed sections on a worker, sharing unchanged native terrain streams.
    pub fn mesh(
        &self,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        named: impl Fn(&str, &str) -> Option<u32>,
        settings: meshing::MeshRenderSettings,
    ) -> Result<BlockMesh, DrawError> {
        self.grid.mesh_changed(
            assets,
            mode,
            named,
            settings,
            self.previous.as_deref(),
            self.dirty.as_ref(),
        )
    }
    /// Claims this generation once; callers must complete every successful claim.
    pub fn claim(&self) -> bool {
        !self.claimed.swap(true, Ordering::AcqRel)
    }

    /// Publishes the worker's whole result, including admission failures.
    pub fn complete(&self, result: Result<BlockMesh, DrawError>) {
        let _ = self.result.set(result.map(Arc::new));
    }

    /// Reads readiness without locking, copying cells or performing meshing work.
    pub fn result(&self) -> Option<&Result<Arc<BlockMesh>, DrawError>> {
        self.result.get()
    }
}

impl BlockGrid {
    /// Validates one edit before copying cells and returns its dense storage index.
    fn edit_index(&self, edit: BlockEdit) -> Result<usize, DrawError> {
        if (0..3).any(|axis| edit.position[axis] >= self.size[axis])
            || edit.cell.palette as usize >= self.palette.len()
            || edit.cell.sky_light.is_some_and(|v| v > MAX_BLOCK_LIGHT)
            || edit.cell.block_light.is_some_and(|v| v > MAX_BLOCK_LIGHT)
        {
            return Err(error(ErrorCode::Invalid, "block edit"));
        }
        let [x, y, z] = edit.position.map(|v| v as usize);
        Ok((x * self.size[2] as usize + z) * self.size[1] as usize + y)
    }

    /// Returns the dense x,z,y index, or air outside the supplied bounds.
    pub fn cell(&self, position: [i32; 3]) -> Option<BlockCell> {
        if (0..3).any(|axis| position[axis] < 0 || position[axis] as u32 >= self.size[axis]) {
            return None;
        }
        let [x, y, z] = position.map(|value| value as usize);
        self.cells
            .get((x * self.size[2] as usize + z) * self.size[1] as usize + y)
            .copied()
    }

    /// Checks all allocations before retaining a grid; expensive mesh building happens later.
    pub(super) fn admit(self) -> Result<Geometry, DrawError> {
        let cells = self
            .size
            .into_iter()
            .try_fold(1usize, |total, value| {
                if value == 0 || value > MAX_BLOCK_EXTENT {
                    return None;
                }
                total.checked_mul(value as usize)
            })
            .ok_or(error(ErrorCode::BudgetExceeded, "block grid extent"))?;
        if cells > MAX_BLOCK_CELLS || self.palette.len() > MAX_BLOCK_PALETTE {
            return Err(error(
                ErrorCode::BudgetExceeded,
                "block grid cells or palette",
            ));
        }
        let strings = self
            .palette
            .iter()
            .try_fold(0usize, |total, state| {
                total.checked_add(match state {
                    BlockState::RuntimeId(_) => 0,
                    BlockState::Named { name, states } => name.len().checked_add(states.len())?,
                })
            })
            .ok_or(error(ErrorCode::BudgetExceeded, "block states"))?;
        if strings > MAX_BLOCK_STATE_BYTES {
            return Err(error(ErrorCode::BudgetExceeded, "block states"));
        }
        if self.cells.len() != cells
            || self.palette.is_empty()
            || self.cells.iter().any(|cell| {
                cell.palette as usize >= self.palette.len()
                    || cell.sky_light.is_some_and(|v| v > MAX_BLOCK_LIGHT)
                    || cell.block_light.is_some_and(|v| v > MAX_BLOCK_LIGHT)
            })
        {
            return Err(error(ErrorCode::Invalid, "block cells or light"));
        }
        let bounds = [[0.; 3], self.size.map(|v| v as f32)];
        Ok(Geometry {
            radius: bounds[1].iter().map(|v| v * v).sum::<f32>().sqrt(),
            bounds,
            bytes: cells * size_of::<BlockCell>()
                + self.palette.len() * size_of::<BlockState>()
                + strings,
            blocks: Some(Arc::new(BlockGeometry {
                grid: self,
                claimed: AtomicBool::new(false),
                result: OnceLock::new(),
                render_error: OnceLock::new(),
                render_ready: AtomicBool::new(false),
                remote: OnceLock::new(),
                context: OnceLock::new(),
                previous: None,
                dirty: None,
            })),
            vertices: Arc::from([]),
            indices: Arc::from([]),
            distances: Arc::from([]),
            closed: false,
            polyline: false,
            expanded_vertices: 0,
            mesh_vertices: 0,
        })
    }

    /// Builds the same section snapshots, neighbour culling, light and packed streams as terrain.
    /// Call only on a worker. Unknown palette entries fail the whole generation.
    pub fn mesh(
        &self,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        named: impl Fn(&str, &str) -> Option<u32>,
        settings: meshing::MeshRenderSettings,
    ) -> Result<BlockMesh, DrawError> {
        self.mesh_changed(assets, mode, named, settings, None, None)
    }

    /// Reuses immutable section streams outside the complete edit dependency set.
    fn mesh_changed(
        &self,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        named: impl Fn(&str, &str) -> Option<u32>,
        settings: meshing::MeshRenderSettings,
        previous: Option<&BlockMesh>,
        dirty: Option<&BTreeSet<[i32; 3]>>,
    ) -> Result<BlockMesh, DrawError> {
        let air = assets
            .air_network_id(mode)
            .ok_or(error(ErrorCode::Unsupported, "terrain registry"))?;
        let palette: Vec<u32> = self
            .palette
            .iter()
            .map(|state| {
                let id = match state {
                    BlockState::RuntimeId(id) => Some(*id),
                    BlockState::Named { name, states } => named(name, states),
                };
                id.filter(|id| assets.is_known(mode, *id))
                    .ok_or(error(ErrorCode::Invalid, "block state"))
            })
            .collect::<Result<_, _>>()?;
        let mut sections = BTreeMap::new();
        let counts = self
            .size
            .map(|v| v.div_ceil(world::SUB_CHUNK_SIDE as u32) as i32);
        for x in 0..counts[0] {
            for z in 0..counts[2] {
                for y in 0..counts[1] {
                    let origin = [x, y, z].map(|v| v * world::SUB_CHUNK_SIDE as i32);
                    let ids = std::array::from_fn(|index| {
                        let local = [
                            (index >> 8) as i32,
                            (index & 15) as i32,
                            ((index >> 4) & 15) as i32,
                        ];
                        self.cell(std::array::from_fn(|axis| origin[axis] + local[axis]))
                            .map_or(air, |cell| palette[cell.palette as usize])
                    });
                    sections.insert([x, y, z], world::SubChunk::from_runtime_ids(&ids));
                }
            }
        }
        let mut result = BlockMesh {
            sections: Vec::new(),
            triangles: 0,
            bytes: 0,
        };
        let bounds = meshing::MeshOutputBounds::new(assets);
        for (&key, section) in &sections {
            if bounds.for_sub_chunk(section, assets, mode) > MAX_BLOCK_MESH_BYTES as u64 {
                return Err(error(ErrorCode::BudgetExceeded, "block section mesh"));
            }
            let origin = key.map(|v| v * world::SUB_CHUNK_SIDE as i32);
            if dirty.is_some_and(|dirty| !dirty.contains(&key)) {
                if let Some(old) = previous
                    .and_then(|old| old.sections.iter().find(|section| section.origin == origin))
                {
                    result.triangles += triangles(&old.mesh);
                    result.bytes += meshing::chunk_publication_byte_len(
                        &old.mesh,
                        &meshing::PackedBiomeRecord::fallback(),
                    ) as usize;
                    result.sections.push(Arc::clone(old));
                }
                continue;
            }
            let mut neighbours = world::MeshNeighbourhood::new(section).with_block_origin(origin);
            for above in key[1] + 2..counts[1] {
                if let Some(upper) = sections.get(&[key[0], above, key[2]]) {
                    neighbours.insert_column_above(above - key[1], upper);
                }
            }
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        let delta = [x, y, z];
                        if let Some(adjacent) = sections.get(&std::array::from_fn(|axis| {
                            key[axis] + i32::from(delta[axis])
                        })) {
                            neighbours.insert(delta, adjacent);
                        }
                    }
                }
            }
            let light = |position: [i32; 3]| {
                let cell = self.cell(std::array::from_fn(|axis| position[axis] + origin[axis]));
                meshing::lighting::MeshLightSample::try_new(
                    cell.and_then(|v| v.block_light).unwrap_or(0),
                    cell.and_then(|v| v.sky_light).unwrap_or(MAX_BLOCK_LIGHT),
                )
                .expect("admitted light")
            };
            let mesh = meshing::mesh_sub_chunk_in_neighbourhood_with_options(
                &meshing::BlockClassifier::new(air),
                assets,
                mode,
                &neighbours,
                &light,
                meshing::MeshOptions {
                    rendering: settings,
                    ..Default::default()
                },
            );
            result.triangles += triangles(&mesh);
            result.bytes +=
                meshing::chunk_publication_byte_len(&mesh, &meshing::PackedBiomeRecord::fallback())
                    as usize;
            if result.triangles > MAX_BLOCK_TRIANGLES || result.bytes > MAX_BLOCK_MESH_BYTES {
                return Err(error(ErrorCode::BudgetExceeded, "block mesh"));
            }
            if !mesh.is_empty() {
                result
                    .sections
                    .push(Arc::new(BlockSection { origin, mesh }));
            }
        }
        if result.triangles > MAX_BLOCK_TRIANGLES || result.bytes > MAX_BLOCK_MESH_BYTES {
            return Err(error(ErrorCode::BudgetExceeded, "block mesh"));
        }
        Ok(result)
    }
}

/// Counts the terrain quads every native stream actually draws.
fn triangles(mesh: &meshing::ChunkMesh) -> usize {
    (mesh.cube_quads().len()
        + mesh.model_draw_refs().len()
        + mesh.transparent_model_draw_refs().len()
        + mesh.liquid_quads().len())
        * 2
}

impl super::Store {
    /// Replaces grid cells atomically and preserves unchanged section output.
    pub fn update_blocks(
        &mut self,
        id: super::Handle,
        edits: Vec<BlockEdit>,
        permissions: DrawingPermissions,
    ) -> Result<(), DrawError> {
        if !permissions.world_draw || !permissions.render_material {
            return Err(error(ErrorCode::Denied, "block geometry"));
        }
        self.check(id)?;
        if edits.len() > MAX_BLOCK_CELLS {
            return Err(error(ErrorCode::BudgetExceeded, "block edits"));
        }
        let current = self.staged.as_ref().unwrap_or(&self.committed);
        let old = Arc::clone(
            current
                .geometry
                .get(&id)
                .ok_or(error(ErrorCode::StaleHandle, "geometry"))?,
        );
        let block = old
            .blocks()
            .ok_or(error(ErrorCode::Invalid, "block geometry"))?;
        let previous = match block.result() {
            Some(Ok(mesh)) => Some(Arc::clone(mesh)),
            Some(Err(error)) => return Err(error.clone()),
            None => match block.remote.get() {
                Some(Ok(())) => None,
                Some(Err(code)) => return Err(error(*code, "block renderer")),
                None => return Err(error(ErrorCode::InUse, "block mesh pending")),
            },
        };
        let mut changes = BTreeMap::new();
        for edit in edits {
            changes.insert(block.grid.edit_index(edit)?, edit);
        }
        changes.retain(|&index, edit| block.grid.cells[index] != edit.cell);
        if changes.is_empty() {
            return Ok(());
        }
        let mut grid = block.grid.clone();
        grid.revision = grid
            .revision
            .checked_add(1)
            .ok_or(error(ErrorCode::BudgetExceeded, "block generations"))?;
        let mut dirty = BTreeSet::new();
        for (index, edit) in changes {
            grid.cells[index] = edit.cell;
            // Neighbour samples cover culling, liquids and AO. Seasonal foliage also
            // reads shelter throughout the same vertical section column.
            let p = edit.position.map(|v| v as i32);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    for sy in 0..grid.size[1].div_ceil(world::SUB_CHUNK_SIDE as u32) as i32 {
                        dirty.insert([
                            (p[0] + dx).div_euclid(world::SUB_CHUNK_SIDE as i32),
                            sy,
                            (p[2] + dz).div_euclid(world::SUB_CHUNK_SIDE as i32),
                        ]);
                    }
                }
            }
        }
        if dirty.is_empty() {
            return Ok(());
        }
        let mut geometry = grid.admit()?;
        let request =
            Arc::get_mut(geometry.blocks.as_mut().expect("block geometry")).expect("new request");
        if block.render_error().is_none()
            && let Some(context) = block.context.get()
        {
            let _ = request.context.set(context.clone());
            request.dirty = previous.as_ref().map(|_| dirty);
            request.previous = previous;
        }
        let bytes = geometry.bytes;
        self.retired.push(old);
        let result = self.edit_blocks(bytes, |resources| {
            resources.geometry.insert(id, Arc::new(geometry));
            resources.geometry_revision += 1;
            Ok(())
        });
        if result.is_err() {
            self.retired.pop();
        }
        result
    }
}
