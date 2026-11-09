//! Recycles publication headers only after all readers release their immutable frame.

use std::sync::Arc;

use crate::UiRenderInput;

#[derive(Debug, Default)]
pub(crate) struct UiPublicationPool {
    retired: Vec<Arc<UiRenderInput>>,
}

impl Clone for UiPublicationPool {
    /// Extracted scene snapshots need the publication, without inheriting the producer's scratch.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for UiPublicationPool {
    /// Reusable storage does not change a scene's observable publication.
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl UiPublicationPool {
    /// Reuses headers for current draw buffers without retaining replaced resources or changing readers.
    pub(crate) fn publish(
        &mut self,
        previous: Option<Arc<UiRenderInput>>,
        input: UiRenderInput,
    ) -> Arc<UiRenderInput> {
        self.retired
            .retain(|retired| retired.shares_draw_buffers(&input));
        if let Some(previous) = previous.filter(|previous| previous.shares_draw_buffers(&input)) {
            self.retired.push(previous);
        }
        if let Some(index) = self
            .retired
            .iter_mut()
            .position(|input| Arc::get_mut(input).is_some())
        {
            let mut reusable = self.retired.swap_remove(index);
            *Arc::get_mut(&mut reusable).expect("unshared publication header") = input;
            reusable
        } else {
            Arc::new(input)
        }
    }
}
