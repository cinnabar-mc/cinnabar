//! The chunk instances marked as changed since the previous preparation, checked against
//! change detection in debug builds.
use super::*;

type AllChunkInstances<'w, 's> = Query<'w, 's, (Entity, &'static ChunkRenderInstance)>;
/// Change detection over every instance, scanned only to check the marked changes.
#[cfg(debug_assertions)]
type ChangedChunkInstances<'w, 's> = Query<'w, 's, Entity, Changed<ChunkRenderInstance>>;
#[cfg(not(debug_assertions))]
type ChangedChunkInstances<'w, 's> = ();
#[derive(SystemParam)]
pub(in crate::chunk) struct ChunkInstanceQueries<'w, 's> {
    pub(super) queries: ParamSet<
        'w,
        's,
        (
            AllChunkInstances<'w, 's>,
            ChangedChunkInstances<'w, 's>,
            Query<'w, 's, &'static mut ChunkRenderInstance>,
        ),
    >,
    changes: Res<'w, ChunkInstanceChanges>,
    pub(super) marked: Local<'s, MarkedChunkInstances>,
}
/// Instances marked since the previous preparation, and what checking them needs.
#[derive(Default)]
pub(in crate::chunk) struct MarkedChunkInstances {
    pub(super) entities: Vec<Entity>,
    /// Instances this system changed itself; change detection reports them next run, but
    /// their uploads already match, so the plan would skip them anyway.
    #[cfg(debug_assertions)]
    pub(super) completed: Vec<Entity>,
    #[cfg(debug_assertions)]
    ran: bool,
}
impl ChunkInstanceQueries<'_, '_> {
    /// Takes the instances inserted or marked since this system last ran.
    ///
    /// Debug builds check them against change detection, except on a system's first run,
    /// where change detection reports every instance.
    pub(super) fn take_marked(&mut self) {
        let marked = &mut *self.marked;
        self.changes.take_into(&mut marked.entities);
        #[cfg(debug_assertions)]
        {
            // Sorted once per run, so a pack reload marking every instance stays linearithmic.
            let mut completed = std::mem::take(&mut marked.completed);
            completed.sort_unstable();
            if std::mem::replace(&mut marked.ran, true) {
                let mut changed = self.queries.p1().iter().collect::<Vec<_>>();
                changed.sort_unstable();
                for entity in &changed {
                    debug_assert!(
                        marked.entities.binary_search(entity).is_ok()
                            || completed.binary_search(entity).is_ok(),
                        "{entity} changed its chunk instance without marking ChunkInstanceChanges"
                    );
                }
                let all_instances = self.queries.p0();
                for entity in &marked.entities {
                    debug_assert!(
                        changed.binary_search(entity).is_ok() || !all_instances.contains(*entity),
                        "{entity} was marked in ChunkInstanceChanges without changing its instance"
                    );
                }
            }
        }
    }
}
