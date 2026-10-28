use super::*;
use crate::local_worlds::test_support::scratch;
#[cfg(unix)]
use crate::local_worlds::test_support::write_script;
#[cfg(unix)]
use std::path::Path;

#[test]
fn console_acknowledgements_require_the_exact_response() {
    let ack = OutputAck::default();
    let (sender, replied) = crossbeam_channel::bounded(1);
    *lock(&ack.0) = Some(("applied", sender));
    for line in [
        "unrelated applied",
        "applied extra",
        "not applied",
        "[INFO] unrelated",
    ] {
        ack.observe(line);
        assert!(matches!(replied.try_recv(), Err(TryRecvError::Empty)));
    }
    ack.observe("[timestamp INFO] applied");
    assert_eq!(replied.try_recv(), Ok(()));
    ack.observe("applied");
    assert_eq!(replied.try_recv(), Ok(()));
}

/// Uses the real child output reader and command wait without a game installation.
#[cfg(unix)]
#[test]
fn console_confirmation_ends_on_reply_exit_cancel_or_timeout() {
    let dir = scratch();
    let binary = dir.path().join("console");
    write_script(
        &binary,
        br#"#!/bin/sh
echo ready
while IFS= read -r line; do
  case "$line" in
    apply) echo '[INFO] applied' ;;
    exit) exit 0 ;;
    stop) exit 0 ;;
  esac
done
"#,
    );
    let start = || {
        let spawned = spawn(
            Command::new(&binary),
            String::new(),
            |line| line.trim() == "ready",
            "console fixture".into(),
            None,
        )
        .expect("spawn");
        await_ready(spawned, test_time::DEFAULT_TIMEOUT, &Cancel::never()).expect("ready")
    };
    let process = start();
    process
        .send_confirmed(
            "apply",
            "applied",
            test_time::DEFAULT_TIMEOUT,
            &Cancel::never(),
        )
        .expect("output confirmation");
    let (handle, cancel) = super::super::cancel_pair();
    drop(handle);
    assert!(matches!(
        process.send_confirmed("apply", "applied", test_time::DEFAULT_TIMEOUT, &cancel),
        Err(WorldError::Cancelled)
    ));
    assert!(
        process
            .send_confirmed("silent", "applied", Duration::ZERO, &Cancel::never())
            .is_err()
    );
    process.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    let process = start();
    assert!(
        process
            .send_confirmed(
                "exit",
                "applied",
                test_time::DEFAULT_TIMEOUT,
                &Cancel::never()
            )
            .is_err()
    );
    process.stop(test_time::DEFAULT_TIMEOUT).expect("stopped");
}

/// A child that announces readiness and exits on its own stop protocol.
#[cfg(unix)]
const FAKE_SERVER: &[u8] =
    b"#!/bin/sh\necho ready\nwhile IFS= read -r line; do\n  [ \"$line\" = stop ] && exit 0\ndone\n";

/// Completion history from the shared process lifecycle.
#[cfg(unix)]
type RecordedEndings = Arc<Mutex<Vec<Ending>>>;

/// Launches `binary` with a hook that records how it ended.
#[cfg(unix)]
fn launch_recorded(
    binary: &Path,
    timeout: Duration,
    cancel: &Cancel,
) -> (Result<Arc<Process>, WorldError>, RecordedEndings) {
    let endings = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&endings);
    let launched = launch(
        Command::new(binary),
        Launch {
            address: String::new(),
            timeout,
            ready: |line| line.trim() == "ready",
            label: "hook fixture".into(),
            on_exit: Some(Box::new(move |ending| lock(&recorded).push(ending))),
        },
        cancel,
    );
    (launched, endings)
}

/// Each launch reports its ending once, before its exit is observable or its failed launch returns.
#[cfg(unix)]
#[test]
fn every_launch_reports_how_it_ended_before_its_exit() {
    let dir = scratch();
    let script = |name: &str, body: &[u8]| {
        let path = dir.path().join(name);
        write_script(&path, body);
        path
    };
    let server = script("server", FAKE_SERVER);
    let (launched, endings) =
        launch_recorded(&server, test_time::DEFAULT_TIMEOUT, &Cancel::never());
    let process = launched.expect("ready");
    assert!(lock(&endings).is_empty());
    process.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    assert_eq!(*lock(&endings), [Ending::Clean]);

    let crash_on_stop = script(
        "crash-on-stop",
        b"#!/bin/sh\necho ready\nwhile IFS= read -r line; do\n  [ \"$line\" = stop ] && exit 134\ndone\n",
    );
    let (launched, endings) =
        launch_recorded(&crash_on_stop, test_time::DEFAULT_TIMEOUT, &Cancel::never());
    let process = launched.expect("ready");
    let _ = process.stop(test_time::DEFAULT_TIMEOUT);
    assert_eq!(*lock(&endings), [Ending::Failed]);

    let crash = script("crash", b"#!/bin/sh\nexit 3\n");
    let (launched, endings) = launch_recorded(&crash, test_time::DEFAULT_TIMEOUT, &Cancel::never());
    assert!(launched.is_err());
    assert_eq!(*lock(&endings), [Ending::Failed]);

    let silent = script(
        "silent",
        b"#!/bin/sh\nwhile IFS= read -r line; do :; done\n",
    );
    let (launched, endings) = launch_recorded(&silent, Duration::ZERO, &Cancel::never());
    assert!(launched.is_err(), "a start timeout");
    assert_eq!(*lock(&endings), [Ending::Failed]);

    let (handle, cancel) = crate::local_worlds::cancel_pair();
    drop(handle);
    let (launched, endings) = launch_recorded(&silent, test_time::DEFAULT_TIMEOUT, &cancel);
    assert!(matches!(launched, Err(WorldError::Cancelled)));
    assert_eq!(*lock(&endings), [Ending::Cancelled]);

    let (launched, endings) = launch_recorded(
        &dir.path().join("missing"),
        test_time::DEFAULT_TIMEOUT,
        &Cancel::never(),
    );
    assert!(launched.is_err());
    assert_eq!(*lock(&endings), [Ending::Unstarted]);
}

/// A cancelled start that ended any way other than its kill, such as a crash, is a failure.
#[cfg(unix)]
#[test]
fn a_cancellation_does_not_hide_a_crash() {
    use std::os::unix::process::ExitStatusExt;

    let killed = ExitStatus::from_raw(libc::SIGKILL);
    let aborted = ExitStatus::from_raw(libc::SIGABRT);
    let exited = ExitStatus::from_raw(134 << 8);
    assert_eq!(ending(Some(killed), true), Ending::Cancelled);
    assert_eq!(ending(Some(killed), false), Ending::Failed);
    for crash in [aborted, exited] {
        assert_eq!(ending(Some(crash), true), Ending::Failed);
    }
    assert_eq!(ending(None, true), Ending::Failed);
    assert_eq!(ending(Some(ExitStatus::from_raw(0)), true), Ending::Clean);
}
