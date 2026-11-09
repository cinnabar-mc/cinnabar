use super::*;

/// Results leave in publication order and never exceed capacity.
#[test]
fn delivers_in_order_within_capacity() {
    let (sender, mailbox) = result_mailbox(2);
    sender.try_send(1).unwrap();
    sender.try_send(2).unwrap();
    assert!(matches!(sender.try_send(3), Err(TrySendError::Full(3))));
    assert_eq!(mailbox.len(), 2);
    assert_eq!(mailbox.try_recv(), Ok(1));
    sender.try_send(3).unwrap();
    assert_eq!(mailbox.try_iter().collect::<Vec<_>>(), [2, 3]);
    assert!(mailbox.is_empty());
}

/// A publication in progress is skipped without waiting and collected by a later receive.
#[test]
fn skips_a_result_whose_publication_is_in_progress() {
    let (sender, mailbox) = result_mailbox(4);
    sender.send(1).unwrap();
    let (holding, held) = bounded(0);
    let (release, released) = bounded::<()>(0);
    let producer = std::thread::spawn(move || {
        let publish = sender.reserve_publication();
        holding.send(()).unwrap();
        let _ = released.recv();
        publish(2);
    });
    held.recv().unwrap();
    let consumer = std::thread::spawn({
        let mailbox = mailbox.clone();
        move || mailbox.try_recv()
    });
    test_time::eventually("the receive returns while publication is held", || {
        consumer.is_finished()
    });
    assert_eq!(consumer.join().unwrap(), Err(TryRecvError::Empty));
    drop(release);
    producer.join().unwrap();
    assert_eq!(mailbox.try_recv(), Ok(1));
    assert_eq!(mailbox.try_recv(), Ok(2));
}

/// A full mailbox holds its sender until a receive frees space.
#[test]
fn full_mailbox_blocks_the_sender_until_space_frees() {
    let (sender, mailbox) = result_mailbox(1);
    sender.send(1).unwrap();
    let producer = std::thread::spawn(move || sender.send(2).is_ok());
    test_time::eventually("the sender waits for space", || {
        mailbox.shared.lock().blocked_senders == 1
    });
    assert_eq!(mailbox.len(), 1);
    assert_eq!(mailbox.try_recv(), Ok(1));
    assert!(producer.join().unwrap());
    assert_eq!(mailbox.try_recv(), Ok(2));
}

/// Dropping the last receiver releases a blocked sender with its result, so workers finish.
#[test]
fn dropping_the_receiver_releases_a_blocked_sender() {
    let (sender, mailbox) = result_mailbox(1);
    sender.send(Arc::new(())).unwrap();
    let retained = Arc::new(());
    let witness = Arc::downgrade(&retained);
    let producer = std::thread::spawn({
        let sender = sender.clone();
        move || sender.send(retained).is_err()
    });
    test_time::eventually("the sender waits for space", || {
        mailbox.shared.lock().blocked_senders == 1
    });
    let observer = mailbox.clone();
    drop(mailbox);
    assert_eq!(
        observer.shared.lock().blocked_senders,
        1,
        "a remaining receiver keeps the mailbox open"
    );
    drop(observer);
    assert!(producer.join().unwrap());
    assert!(witness.upgrade().is_none(), "the refused result is dropped");
    assert!(sender.send(Arc::new(())).is_err());
}

/// Closing a mailbox releases queued resources even while producers retain their handles.
#[test]
fn dropping_the_receiver_releases_queued_results() {
    let (sender, mailbox) = result_mailbox(1);
    let retained = Arc::new(());
    let witness = Arc::downgrade(&retained);
    sender.send(retained).unwrap();
    let observer = mailbox.clone();
    drop(mailbox);
    assert!(witness.upgrade().is_some());
    drop(observer);
    assert!(witness.upgrade().is_none());
    assert!(sender.send(Arc::new(())).is_err());
}

/// Each publication rings the doorbell a drained consumer sleeps on.
#[test]
fn publication_rings_the_doorbell() {
    let (sender, mailbox) = result_mailbox(4);
    assert!(mailbox.doorbell().try_recv().is_err());
    sender.send(1).unwrap();
    sender.send(2).unwrap();
    assert!(mailbox.doorbell().try_recv().is_ok());
    assert!(mailbox.doorbell().try_recv().is_err());
    assert_eq!(mailbox.recv_timeout(std::time::Duration::ZERO), Ok(1));
}
