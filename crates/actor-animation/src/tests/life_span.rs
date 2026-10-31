use super::*;

#[test]
fn life_span_reads_the_synced_lifetime_and_never_goes_negative() {
    let key = ActorDataKey::DataLifetimeTicks.wire_value();
    for (metadata, expected) in [
        (Some(ActorMetadataValue::Int(22)), 22.0),
        (Some(ActorMetadataValue::Int(-1)), 0.0),
        (None, 0.0),
    ] {
        let actor = actor_with_metadata(metadata.map(|value| (key, value)).into_iter().collect());
        assert_eq!(
            read(&actor, &ActorTickInput::default(), 0, "query.life_span"),
            expected
        );
    }
}
