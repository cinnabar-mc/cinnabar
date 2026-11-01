//! Publication reads resource identities and bounded metadata, never mesh or shader contents.

use super::*;

/// Checks only the new import's segment while its primitive data is already being admitted.
pub(super) fn matches_next(committed: &Primitives, staged: &Primitives, next: &Primitives) -> bool {
    /// Matches one incoming list without revisiting any earlier import's elements.
    fn segment<T: PartialEq>(committed: &[T], offset: usize, next: &[T]) -> bool {
        next.is_empty() || committed.get(offset..offset + next.len()) == Some(next)
    }
    segment(&committed.decals, staged.decals.len(), &next.decals)
        && segment(&committed.ribbons, staged.ribbons.len(), &next.ribbons)
        && segment(&committed.beams, staged.beams.len(), &next.beams)
        && segment(
            &committed.billboards,
            staged.billboards.len(),
            &next.billboards,
        )
}

/// Compares pass revisions and mutable parameters without walking shader source strings.
pub(super) fn same_passes(current: &[Pass], next: &[Pass]) -> bool {
    current.len() == next.len()
        && current.iter().zip(next).all(|(current, next)| {
            current.revision == next.revision
                && current.order == next.order
                && current.enabled == next.enabled
                && current.params == next.params
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies distinct decals without requiring a shader or a component fixture.
    fn primitives(centers: &[f32]) -> Primitives {
        Primitives {
            decals: centers
                .iter()
                .map(|x| mod_render::Decal {
                    center: [*x, 0., 0.],
                    radius: 1.,
                    color: [1.; 4],
                    progress: 0.,
                    style: mod_render::DecalStyle::Disc,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn primitive_admission_preserves_identity_across_differently_partitioned_imports() {
        let mut render = RenderState::new();
        render.append_primitives(primitives(&[1., 2.])).unwrap();
        render.commit_primitives();
        let committed = Arc::clone(&render.output().primitives);
        let generation = render.generation();
        render.begin_frame();
        render.append_primitives(primitives(&[1.])).unwrap();
        render.append_primitives(primitives(&[2.])).unwrap();
        render.commit_primitives();
        assert!(Arc::ptr_eq(&committed, &render.output().primitives));
        assert_eq!(render.generation(), generation);
        render.begin_frame();
        render.append_primitives(primitives(&[1.])).unwrap();
        render.commit_primitives();
        assert!(!Arc::ptr_eq(&committed, &render.output().primitives));
        assert_eq!(render.output().primitives.decals.len(), 1);
    }

    #[test]
    fn primitive_publication_uses_admission_changes_instead_of_rechecking_contents() {
        let mut render = RenderState::new();
        render.append_primitives(primitives(&[1.])).unwrap();
        render.commit_primitives();
        let committed = Arc::clone(&render.output().primitives);
        render.begin_frame();
        render.append_primitives(primitives(&[2.])).unwrap();
        render.commit_primitives();
        assert!(!Arc::ptr_eq(&committed, &render.output().primitives));
        assert_eq!(render.output().primitives.decals[0].center[0], 2.);
        render.begin_frame();
        render.commit_primitives();
        assert!(render.output().primitives.is_empty());
    }
}
