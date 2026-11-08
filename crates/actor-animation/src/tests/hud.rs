use super::*;

#[test]
fn hud_pose_advances_crawling_animation_independently_of_first_person() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping HUD pose test: missing entity fixture directory {}",
                root.display()
            );
            return;
        }
        Err(error) => panic!("read entity fixture directory {}: {error}", root.display()),
    };
    let Some(path) = entries
        .map(|entry| entry.expect("read entity fixture entry").path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
    else {
        eprintln!(
            "skipping HUD pose test: missing entity fixture {}/*.mcbeent",
            root.display()
        );
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping HUD pose test: missing entity fixture {}",
                path.display()
            );
            return;
        }
        Err(error) => panic!("read entity fixture {}: {error}", path.display()),
    };
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    let mut actor = actor_with_metadata(HashMap::from([(
        ActorDataKey::FlagsTwo.wire_value(),
        ActorMetadataValue::FlagsExtended(1 << (ActorFlag::Crawling.wire_value() - u64::BITS)),
    )]));
    actor.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "Offline".into(),
    };
    let mut first = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut third = ActorAnimationStore::with_assets(assets);
    first.insert(1, 0, &actor);
    third.insert(1, 0, &actor);
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut previous_hud = None;
    for tick in 0..4 {
        // The pinned crawl clip advances from modified_distance_moved, so this
        // fixture must move before comparing successive HUD keyframes.
        actors.get_mut(&1).unwrap().position[0] = tick as f32 * 0.2;
        first.advance_tick(&actors, None, Some(1), true, false, |_| ActorTickContext {
            is_local_first_person: true,
            ..Default::default()
        });
        third.advance_tick(&actors, None, Some(1), true, false, |_| {
            ActorTickContext::default()
        });
        let hud = first.ui_pose(1).expect("full body pose");
        assert_eq!(hud, third.ui_pose(1).unwrap(), "HUD tick {tick}");
        if let Some(previous) = previous_hud {
            assert_ne!(hud, previous, "the crawling keyframes must advance");
        }
        previous_hud = Some(hud.to_vec());
    }
    let hud = first.ui_pose(1).expect("full body pose");
    assert!(!hud.is_empty());
    assert_eq!(hud, third.ui_pose(1).unwrap());
    assert_ne!(hud, first.get(1).unwrap().current);
}
