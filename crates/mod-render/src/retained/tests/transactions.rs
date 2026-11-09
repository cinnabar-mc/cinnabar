use super::*;

/// Uses an exact incarnation so removing a runtime ID cannot remove its replacement.
fn actor() -> EntityKey {
    EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision: 1,
    }
}

#[test]
fn culling_does_not_delete_new_entity_handles_or_their_drawings() {
    for despawn in [false, true] {
        let mut native = Store::default();
        native.bind_session(actor().session, actor().dimension);
        native.reconcile_entities(vec![actor()], &[]).unwrap();
        let mut transaction = native.transaction();
        let entity = transaction.observe_entities().unwrap()[0].0;
        let geometry = transaction.create_geometry(mesh()).unwrap();
        let mut instance = drawing(geometry);
        instance.anchor = Anchor::Entity(EntityAnchor {
            entity,
            body_rotation: true,
        });
        let instance = transaction
            .create(instance, DrawingPermissions::ALL)
            .unwrap();
        assert_eq!(native.entity_keys(), [actor()]);
        native.reconcile_entities(Vec::new(), &[]).unwrap();
        if despawn {
            native.reconcile_entities(Vec::new(), &[actor()]).unwrap();
        }
        native.accept(transaction);
        assert_eq!(native.snapshot().entities.contains_key(&entity), !despawn);
        assert_eq!(
            native.snapshot().instances.contains_key(&instance),
            !despawn
        );
    }
}

#[test]
fn discarded_transactions_do_not_recycle_tokens_or_retire_committed_resources() {
    let mut native = Store::default();
    let geometry = native.create_geometry(mesh()).unwrap();
    native.commit();
    let committed = Arc::clone(native.snapshot());
    let mut transaction = native.transaction();
    transaction.remove(geometry).unwrap();
    let discarded = transaction.create_geometry(mesh()).unwrap();
    drop(transaction);
    assert!(Arc::ptr_eq(&committed, native.snapshot()));
    let next = native.create_geometry(mesh()).unwrap();
    assert_ne!(discarded, next);
    assert_eq!(
        native
            .create(drawing(discarded), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    native.commit();
    assert!(native.snapshot().geometry.contains_key(&geometry));
    assert!(native.snapshot().geometry.contains_key(&next));
    assert!(!native.snapshot().geometry.contains_key(&discarded));
}

#[test]
fn successful_delta_does_not_replace_newer_native_entity_state() {
    let mut native = Store::default();
    native.bind_session(actor().session, actor().dimension);
    native.reconcile_entities(vec![actor()], &[]).unwrap();
    native.observe_entities().unwrap();
    native.commit();
    let mut transaction = native.transaction();
    let geometry = transaction.create_geometry(mesh()).unwrap();
    native.reconcile_entities(Vec::new(), &[actor()]).unwrap();
    native.accept(transaction);
    assert!(native.snapshot().geometry.contains_key(&geometry));
    assert!(native.snapshot().entities.is_empty());
}
