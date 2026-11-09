//! One coalescing inventory worker per menu owner; input never waits for disk work.
use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use launcher::{install_layout::InstallLayout, menu::settings_storage::StorageView};
use std::io;

#[derive(Debug)]
struct Request {
    generation: u64,
    layout: InstallLayout,
}

/// Keeps only the latest queued scan and completed inventory. Dropping it never joins the worker.
#[derive(Debug)]
pub struct StorageReader {
    requests: Sender<Request>,
    pending: Receiver<Request>,
    completed: Receiver<(u64, StorageView)>,
    generation: u64,
}

impl StorageReader {
    /// Starts one idle worker that measures storage only when requested.
    pub fn new() -> io::Result<Self> {
        Self::with_reader(|layout| super::read_storage(&layout))
    }

    /// Owns the scanner on its worker, allowing tests to control completion without clocks.
    fn with_reader(
        read: impl Fn(InstallLayout) -> StorageView + Send + 'static,
    ) -> io::Result<Self> {
        let (requests, pending) = bounded::<Request>(1);
        let (finished, completed) = bounded(1);
        let worker_requests = pending.clone();
        let stale_results = completed.clone();
        std::thread::Builder::new()
            .name("storage-inventory".into())
            .spawn(move || {
                while let Ok(request) = worker_requests.recv() {
                    let view = read(request.layout);
                    if replace(&finished, &stale_results, (request.generation, view)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests,
            pending,
            completed,
            generation: 0,
        })
    }

    /// Supersedes a queued scan without blocking input or starting another worker.
    pub fn request(&mut self, layout: InstallLayout) -> io::Result<()> {
        self.generation = self.generation.wrapping_add(1);
        replace(
            &self.requests,
            &self.pending,
            Request {
                generation: self.generation,
                layout,
            },
        )
        .map_err(|_| io::Error::other("Could not load storage. Reopen Storage to retry."))
    }

    /// Returns only an inventory for the latest request; obsolete completions are discarded.
    pub fn poll(&self) -> io::Result<Option<StorageView>> {
        match self.completed.try_recv() {
            Ok((generation, view)) => Ok((generation == self.generation).then_some(view)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::Error::other(
                "Could not load storage. Reopen Storage to retry.",
            )),
        }
    }
}

/// Replaces the single queued value while a consumer may be taking its predecessor.
fn replace<T>(sender: &Sender<T>, queued: &Receiver<T>, value: T) -> Result<(), TrySendError<T>> {
    match sender.try_send(value) {
        Err(TrySendError::Full(value)) => {
            let _ = queued.try_recv();
            sender.try_send(value)
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::settings_storage::StorageItem;

    #[test]
    fn requests_do_not_wait_for_a_scan_and_only_the_latest_inventory_is_published() {
        let (entered, started) = bounded(1);
        let (release, blocked) = bounded(1);
        let mut reader = StorageReader::with_reader(move |layout| {
            if layout.user_data_root.to_string_lossy() == "first" {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
            }
            StorageView {
                cached: vec![StorageItem {
                    name: layout.user_data_root.to_string_lossy().into_owned(),
                    ..Default::default()
                }],
                ..Default::default()
            }
        })
        .unwrap();
        let mut layout = launcher::test_support::scratch("storage-worker");
        layout.user_data_root = "first".into();
        reader.request(layout.clone()).unwrap();
        test_time::eventually("storage scan started", || started.try_recv().is_ok());
        assert!(reader.poll().unwrap().is_none());
        layout.user_data_root = "superseded".into();
        reader.request(layout.clone()).unwrap();
        layout.user_data_root = "latest".into();
        reader.request(layout).unwrap();
        assert!(reader.poll().unwrap().is_none());
        release.send(()).unwrap();
        let mut inventory = None;
        test_time::eventually("latest storage inventory", || {
            inventory = reader.poll().unwrap();
            inventory.is_some()
        });
        assert_eq!(inventory.unwrap().cached[0].name, "latest");
    }
}
