//! The install's Java Edition accounts on their own worker. They do not belong to the Bedrock
//! account, so answers are published whatever the Bedrock sign-in does meanwhile.

use {super::*, launcher::java_accounts::JavaAccounts};

/// Runs actions in order on a dedicated runtime until the sender is dropped. Each answer,
/// or `Err` when the core could not be asked, replaces the last one.
pub(super) fn start(
    socket_dir: PathBuf,
    shared: Arc<Mutex<Snapshot>>,
) -> tokio::sync::mpsc::UnboundedSender<bridge::JavaAccountAction> {
    let (sender, mut actions) = tokio::sync::mpsc::unbounded_channel::<bridge::JavaAccountAction>();
    thread::spawn(move || {
        let runtime = runtime();
        while let Some(action) = actions.blocking_recv() {
            let answer = runtime.as_ref().and_then(|runtime| {
                runtime.block_on(async {
                    tokio::time::timeout(
                        profile_worker::RESPONSE_TIMEOUT,
                        bridge::java_accounts(&socket_dir, &action),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                })
            });
            publish(&shared, |snapshot| {
                snapshot.java_accounts = Some(answer.map(JavaAccounts::from_reply).ok_or(()));
            });
        }
    });
    sender
}

impl LauncherAccount {
    /// Queues one action for the Java accounts worker.
    pub(super) fn request_java_accounts_control(
        &mut self,
        action: bridge::JavaAccountAction,
    ) -> bool {
        self.java_accounts.send(action).is_ok()
    }

    /// Takes the latest answer once.
    pub(super) fn java_accounts_control(&mut self) -> Option<Result<JavaAccounts, ()>> {
        self.with(|snapshot| snapshot.java_accounts.take())
    }
}
