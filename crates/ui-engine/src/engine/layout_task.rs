//! Background layout with bounded access to the engine's texture metadata.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
    task::Poll,
};

use assets::RuntimeFontCatalog;
use json_ui::{AsepriteFrame, LayoutEnv, TextureMeta, TextureSource};
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache};

use super::{FormEngine, text_paint::Measure};
use crate::{text_metrics::TextMetrics, textures::Textures};

/// A detached translation lookup that can be read by the layout worker.
pub type Translate = dyn Fn(&str) -> Option<Arc<str>> + Send + Sync;

type Frames = Option<Arc<[AsepriteFrame]>>;

/// Immutable text resources for a layout that must not borrow live presentation state.
pub struct LayoutResources {
    pub font: Arc<RuntimeFontCatalog>,
    pub layouts: TextLayoutCache,
    pub translate: Arc<Translate>,
}

/// Main-thread metadata requests served per closed-screen frame. Binding, text measurement
/// and control placement run entirely on the worker.
pub const METADATA_REQUESTS_PER_FRAME: usize = 8;

enum Event<T> {
    Texture(String, mpsc::SyncSender<Option<TextureMeta>>),
    Frames(String, mpsc::SyncSender<Frames>),
    Done(T),
}

/// One worker's completion and metadata queue; dropping it cancels further metadata reads.
pub struct LayoutTask<T> {
    events: Mutex<mpsc::Receiver<Event<T>>>,
}

struct WorkerTextures<T> {
    events: mpsc::Sender<Event<T>>,
    metadata: RefCell<HashMap<String, Option<TextureMeta>>>,
    frames: RefCell<HashMap<String, Frames>>,
    revision: u64,
}

impl<T> TextureSource for WorkerTextures<T> {
    /// Reads each texture once from the owning engine, then reuses that answer on the worker.
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        if let Some(value) = self.metadata.borrow().get(path) {
            return *value;
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.events
            .send(Event::Texture(path.to_owned(), reply))
            .ok()?;
        let value = result.recv().ok()?;
        self.metadata.borrow_mut().insert(path.to_owned(), value);
        value
    }

    /// Keeps the source epoch that this worker's measurements started against.
    fn metadata_revision(&self) -> u64 {
        self.revision
    }

    /// Retains immutable animation sidecars without sharing the engine's atlas lock.
    fn aseprite_frames(&self, path: &str) -> Option<Arc<[AsepriteFrame]>> {
        if let Some(value) = self.frames.borrow().get(path) {
            return value.clone();
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.events
            .send(Event::Frames(path.to_owned(), reply))
            .ok()?;
        let value = result.recv().ok()?;
        self.frames
            .borrow_mut()
            .insert(path.to_owned(), value.clone());
        value
    }
}

impl<T: Send + 'static> LayoutTask<T> {
    /// Starts one owned layout. Failure to start leaves the visible paint's normal fallback.
    pub fn spawn(
        resources: LayoutResources,
        metrics: TextMetrics,
        content: [f32; 2],
        revision: u64,
        work: impl FnOnce(&LayoutEnv, [f64; 2]) -> T + Send + 'static,
    ) -> Option<Self> {
        let (events, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("inventory-layout".into())
            .spawn(move || {
                let mut layouts = resources.layouts;
                let cache = RefCell::new(&mut layouts);
                let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
                let measure = Measure {
                    layouts: &cache,
                    font: &resources.font,
                    metrics,
                    px,
                    translate: &*resources.translate,
                };
                let textures = WorkerTextures {
                    events: events.clone(),
                    metadata: RefCell::default(),
                    frames: RefCell::default(),
                    revision,
                };
                let env = LayoutEnv {
                    text: &measure,
                    textures: &textures,
                };
                let root = content.map(|size| f64::from(size / px));
                let result = work(&env, root);
                let _ = events.send(Event::Done(result));
            })
            .ok()?;
        Some(Self {
            events: Mutex::new(receiver),
        })
    }

    /// Services a fixed number of metadata reads without waiting for the worker.
    pub fn poll(&mut self, engine: &FormEngine) -> Poll<Option<T>> {
        self.poll_with(|event| Self::service(engine, event))
    }

    /// Drains only this frame's allowance, including a completion that is already queued.
    fn poll_with(&mut self, mut service: impl FnMut(Event<T>) -> Option<T>) -> Poll<Option<T>> {
        for _ in 0..METADATA_REQUESTS_PER_FRAME {
            let event = match self.events.get_mut().unwrap().try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => return Poll::Pending,
                Err(mpsc::TryRecvError::Disconnected) => return Poll::Ready(None),
            };
            if let Some(result) = service(event) {
                return Poll::Ready(Some(result));
            }
        }
        Poll::Pending
    }

    /// Finishes outstanding work when the screen opens, servicing its remaining metadata.
    pub fn finish(&mut self, engine: &FormEngine) -> Option<T> {
        while let Ok(event) = self.events.get_mut().unwrap().recv() {
            if let Some(result) = Self::service(engine, event) {
                return Some(result);
            }
        }
        None
    }

    /// Answers one worker query while holding the atlas only for that lookup.
    fn service(engine: &FormEngine, event: Event<T>) -> Option<T> {
        if let Event::Done(result) = event {
            return Some(result);
        }
        let atlas = engine.textures.lock();
        let view = Textures {
            assets: engine.assets(),
            set: &engine.textures,
            atlas: &atlas,
            images: None,
        };
        match event {
            Event::Texture(path, reply) => {
                let _ = reply.send(view.texture(&path));
            }
            Event::Frames(path, reply) => {
                let _ = reply.send(view.aseprite_frames(&path));
            }
            Event::Done(_) => unreachable!(),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_poll_bounds_metadata_work_and_keeps_the_remaining_requests() {
        let (sender, events) = mpsc::channel::<Event<usize>>();
        let mut replies = Vec::new();
        let total = METADATA_REQUESTS_PER_FRAME * 2 + 1;
        for index in 0..total {
            let (reply, result) = mpsc::sync_channel(1);
            sender
                .send(Event::Texture(index.to_string(), reply))
                .unwrap();
            replies.push(result);
        }
        sender.send(Event::Done(total)).unwrap();
        let mut task = LayoutTask {
            events: Mutex::new(events),
        };
        let mut served = 0;
        for frame in 0..3 {
            let before = served;
            let result = task.poll_with(|event| match event {
                Event::Texture(_, reply) => {
                    served += 1;
                    reply.send(None).unwrap();
                    None
                }
                Event::Done(value) => Some(value),
                Event::Frames(_, _) => unreachable!(),
            });
            assert!(served - before <= METADATA_REQUESTS_PER_FRAME);
            assert_eq!(
                result,
                if frame == 2 {
                    Poll::Ready(Some(total))
                } else {
                    Poll::Pending
                }
            );
        }
        assert_eq!(served, total);
        assert!(
            replies
                .into_iter()
                .all(|reply| reply.try_recv() == Ok(None))
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn layout_work_runs_off_the_callers_thread() {
        let caller = std::thread::current().id();
        let resources = LayoutResources {
            font: crate::test_support::fixture_font(),
            layouts: TextLayoutCache::new(16, 1024 * 1024),
            translate: Arc::new(|_| None),
        };
        let metrics =
            TextMetrics::for_viewport([320, 240], ui::DpiScale::new(1.0).unwrap(), Some(1));
        let mut task = LayoutTask::spawn(resources, metrics, [320., 240.], 0, move |_, root| {
            assert_ne!(std::thread::current().id(), caller);
            root
        })
        .unwrap();
        test_time::eventually("background layout completion", || {
            matches!(
                task.poll_with(|event| match event {
                    Event::Done(root) => Some(root),
                    _ => unreachable!(),
                }),
                Poll::Ready(Some([320., 240.]))
            )
        });
    }
}
