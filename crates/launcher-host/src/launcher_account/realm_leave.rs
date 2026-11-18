//! Leaving a joined Realm on its own worker. A submitted leave is never cancelled, because the
//! service may already have applied it; its outcome is always published to the requesting account.

use {super::*, crate::account_control::RealmLeaveResponse};

/// One confirmed leave: the menu's request ticket and the Realm's service id.
pub(super) struct Request {
    pub(super) ticket: u64,
    pub(super) realm_id: u32,
}

/// Runs requests in order on a dedicated runtime until the sender is dropped.
pub(super) fn start(
    socket_dir: PathBuf,
    shared: Arc<Mutex<Snapshot>>,
) -> tokio::sync::mpsc::UnboundedSender<Request> {
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel::<Request>();
    thread::spawn(move || {
        let runtime = runtime();
        while let Some(Request { ticket, realm_id }) = requests.blocking_recv() {
            let generation = auth_generation(&shared);
            let left = runtime.as_ref().is_some_and(|runtime| {
                runtime.block_on(async {
                    matches!(
                        tokio::time::timeout(
                            profile_worker::RESPONSE_TIMEOUT,
                            bridge::leave_realm(&socket_dir, realm_id),
                        )
                        .await,
                        Ok(Ok(()))
                    )
                })
            });
            publish_account(&shared, generation, |snapshot| {
                finish(snapshot, ticket, realm_id, left)
            });
        }
    });
    sender
}

/// Publishes a leave's outcome. Success drops the Realm from the catalog at once, retires catalog
/// replies started before the leave so they cannot restore it, and asks for a fresh list.
fn finish(snapshot: &mut Snapshot, ticket: u64, realm_id: u32, left: bool) {
    if left {
        let target = format!("{}{realm_id}", bridge::contract::REALM_TARGET_PREFIX);
        snapshot.realm_catalog_revision = snapshot.realm_catalog_revision.wrapping_add(1);
        if let Some(realms) = &mut snapshot.realms {
            realms.retain(|realm| realm.target != target);
        }
        if let Some(wake) = &snapshot.catalog_wake {
            let _ = wake.try_send(());
        }
    }
    snapshot.realm_leave = Some((ticket, if left { Ok(()) } else { Err(()) }));
}

impl LauncherAccount {
    /// Queues one confirmed leave for the account worker.
    pub(super) fn request_realm_leave_control(&mut self, ticket: u64, realm_id: u32) -> bool {
        self.realm_leave.send(Request { ticket, realm_id }).is_ok()
    }

    /// Takes the latest outcome once, leaving stale-ticket rejection to the menu.
    pub(super) fn realm_leave_control(&mut self) -> Option<RealmLeaveResponse> {
        self.with(|snapshot| snapshot.realm_leave.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalog holding two Realms and a revision a pending catalog reply captured.
    fn snapshot() -> Snapshot {
        let realm = |target: &str| -> Realm {
            serde_json::from_value(serde_json::json!({
                "name": "Realm", "state": "OPEN", "target": target, "member": true
            }))
            .unwrap()
        };
        Snapshot {
            realms: Some(vec![realm("realm_id/7"), realm("realm_id/8")]),
            realm_catalog_revision: 4,
            ..Default::default()
        }
    }

    #[test]
    fn leaving_removes_only_that_realm_and_retires_older_catalog_replies() {
        let mut snapshot = snapshot();
        finish(&mut snapshot, 3, 7, true);
        let targets: Vec<_> = snapshot
            .realms
            .as_ref()
            .unwrap()
            .iter()
            .map(|r| r.target.as_str())
            .collect();
        assert_eq!(targets, ["realm_id/8"]);
        assert_ne!(snapshot.realm_catalog_revision, 4);
        assert_eq!(snapshot.realm_leave, Some((3, Ok(()))));
    }

    #[test]
    fn a_failed_leave_keeps_the_catalog() {
        let mut snapshot = snapshot();
        finish(&mut snapshot, 3, 7, false);
        assert_eq!(snapshot.realms.as_ref().unwrap().len(), 2);
        assert_eq!(snapshot.realm_catalog_revision, 4);
        assert_eq!(snapshot.realm_leave, Some((3, Err(()))));
    }

    #[test]
    fn an_outcome_for_a_retired_account_is_dropped() {
        let shared = Mutex::new(snapshot());
        let generation = auth_generation(&shared);
        shared.lock().unwrap().retire_account_data();
        publish_account(&shared, generation, |snapshot| finish(snapshot, 3, 7, true));
        let snapshot = shared.lock().unwrap();
        assert!(snapshot.realm_leave.is_none());
        assert!(snapshot.realms.is_none());
    }
}
