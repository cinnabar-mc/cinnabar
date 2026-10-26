use super::*;

fn effect(effect_id: i32, amplifier: i32, remaining_ticks: Option<u64>) -> MobEffectEntry {
    MobEffectEntry {
        effect_id,
        amplifier,
        ambient: false,
        remaining_ticks,
    }
}

fn english(key: &str) -> Option<String> {
    Some(
        match key {
            "potion.moveSpeed" => "Speed",
            "potion.regeneration" => "Regeneration",
            "potion.damageBoost" => "Strength",
            "potion.potency.1" => "II",
            "potion.potency.5" => "VI",
            "effect.duration.infinite" => "∞",
            _ => return None,
        }
        .to_owned(),
    )
}

#[test]
fn timer_shows_minutes_and_two_digit_seconds() {
    assert_eq!(format_tick_duration(0), "0:00");
    assert_eq!(format_tick_duration(19), "0:00");
    assert_eq!(format_tick_duration(20 * 9), "0:09");
    assert_eq!(format_tick_duration(20 * 75), "1:15");
    assert_eq!(format_tick_duration(20 * 3_600), "60:00");
}

#[test]
fn rows_follow_effect_ids_with_potency_and_infinite_timers() {
    let rows = effect_rows(
        &[
            effect(10, 5, Some(20 * 45)),
            effect(1, 1, None),
            effect(5, 0, Some(20 * 125)),
        ],
        &english,
    );
    let shown: Vec<_> = rows
        .iter()
        .map(|row| (row.name.as_str(), row.timer.as_str(), row.texture.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Speed II", "∞", "textures/ui/speed_effect"),
            ("Strength", "2:05", "textures/ui/strength_effect"),
            ("Regeneration VI", "0:45", "textures/ui/regeneration_effect"),
        ]
    );
}

#[test]
fn iconless_expired_and_unknown_effects_are_not_listed() {
    // Instant health has no persistent icon; id 99 is unregistered.
    let rows = effect_rows(
        &[
            effect(6, 0, Some(1)),
            effect(1, 0, Some(0)),
            effect(99, 0, None),
            effect(1, 7, Some(40)),
        ],
        &english,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Speed", "potency past VI shows the bare name");
}
