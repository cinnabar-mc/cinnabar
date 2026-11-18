use super::*;

/// Appends a named tag header in fixed little-endian NBT.
fn named(out: &mut Vec<u8>, tag: u8, name: &str) {
    out.push(tag);
    out.extend((name.len() as u16).to_le_bytes());
    out.extend(name.as_bytes());
}

/// Encodes an explosion compound body, end tag included.
fn explosion_body(out: &mut Vec<u8>, shape: u8, colors: &[u8], fades: &[u8], trail: bool) {
    named(out, TAG_BYTE, "FireworkType");
    out.push(shape);
    named(out, TAG_BYTE_ARRAY, "FireworkColor");
    out.extend((colors.len() as i32).to_le_bytes());
    out.extend(colors);
    named(out, TAG_BYTE_ARRAY, "FireworkFade");
    out.extend((fades.len() as i32).to_le_bytes());
    out.extend(fades);
    named(out, TAG_BYTE, "FireworkTrail");
    out.push(u8::from(trail));
    named(out, TAG_BYTE, "FireworkFlicker");
    out.push(0);
    out.push(0);
}

/// Wraps root compound entries in the item user-data header.
fn user_data(entries: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut out = vec![0xff, 0xff, 1, TAG_COMPOUND, 0, 0];
    entries(&mut out);
    out.push(0);
    out
}

#[test]
fn rocket_reads_flight_and_every_explosion_in_order() {
    let data = user_data(|out| {
        named(out, TAG_COMPOUND, "Fireworks");
        named(out, TAG_LIST, "Explosions");
        out.push(TAG_COMPOUND);
        out.extend(2_i32.to_le_bytes());
        explosion_body(out, 0, &[12], &[], false);
        explosion_body(out, 4, &[1, 14], &[15], true);
        named(out, TAG_BYTE, "Flight");
        out.push(3);
        out.push(0);
    });
    let fireworks = item_fireworks(&data).unwrap();
    assert_eq!(fireworks.flight, Some(3));
    assert_eq!(
        fireworks.explosions,
        [
            FireworkExplosion {
                shape: 0,
                colors: vec![12],
                ..FireworkExplosion::default()
            },
            FireworkExplosion {
                shape: 4,
                colors: vec![1, 14],
                fade_colors: vec![15],
                trail: true,
                flicker: false,
            },
        ]
    );
}

#[test]
fn star_reads_its_fireworks_item_compound() {
    let data = user_data(|out| {
        named(out, TAG_COMPOUND, "FireworksItem");
        explosion_body(out, 2, &[13], &[], false);
    });
    let star = item_firework_star(&data).unwrap();
    assert_eq!(star.shape, 2);
    assert_eq!(star.colors, [13]);
    assert!(item_fireworks(&data).is_none());
}

#[test]
fn missing_and_truncated_firework_data_reads_as_absent() {
    assert!(item_fireworks(&[]).is_none());
    assert!(item_firework_star(&user_data(|_| {})).is_none());
    let mut truncated = user_data(|out| {
        named(out, TAG_COMPOUND, "FireworksItem");
        explosion_body(out, 1, &[3, 4], &[], false);
    });
    truncated.truncate(30);
    assert!(item_firework_star(&truncated).is_none());
}
