//! An in-memory server for tests that need an open world without a server binary.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};

use super::{
    Cancel, lock,
    process::{HostGrant, Instance, Runner, StartSpec},
    world::WorldError,
};

/// Starts a [`HeldServer`] for every world at once.
pub(super) struct HeldRunner;

impl Runner for HeldRunner {
    fn start(&self, _spec: &StartSpec, _cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError> {
        let (exit, exited) = crossbeam_channel::bounded(0);
        Ok(Arc::new(HeldServer {
            exit: Mutex::new(Some(exit)),
            exited,
        }))
    }

    fn host_grant(&self) -> HostGrant {
        HostGrant::None
    }
}

/// A server that runs until it is asked to stop, then exits cleanly at once.
struct HeldServer {
    exit: Mutex<Option<Sender<()>>>,
    exited: Receiver<()>,
}

impl Instance for HeldServer {
    fn address(&self) -> &str {
        "127.0.0.1:1"
    }

    fn request_stop(&self) {
        lock(&self.exit).take();
    }

    fn stop(&self, _timeout: Duration) -> Result<(), String> {
        self.request_stop();
        Ok(())
    }

    fn exited(&self) -> Receiver<()> {
        self.exited.clone()
    }

    fn exited_cleanly(&self) -> bool {
        lock(&self.exit).is_none()
    }

    fn grant_host(&self, _xuid: Option<&str>, _cancel: &Cancel) -> Result<(), WorldError> {
        Ok(())
    }
}
