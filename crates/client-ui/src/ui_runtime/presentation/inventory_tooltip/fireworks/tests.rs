use super::super::tooltip_lines;
use super::*;
use protocol::NetworkItemStack;

/// Appends a named tag header in fixed little-endian NBT.
fn named(out: &mut Vec<u8>, tag: u8, name: &str) {
    out.push(tag);
    out.extend((name.len() as u16).to_le_bytes());
    out.extend(name.as_bytes());
}

/// Encodes an explosion compound body, end tag included.
fn explosion(out: &mut Vec<u8>, shape: u8, colors: &[u8], fades: &[u8], trail: bool) {
    named(out, 1, "FireworkType");
    out.push(shape);
    named(out, 7, "FireworkColor");
    out.extend((colors.len() as i32).to_le_bytes());
    out.extend(colors);
    if !fades.is_empty() {
        named(out, 7, "FireworkFade");
        out.extend((fades.len() as i32).to_le_bytes());
        out.extend(fades);
    }
    named(out, 1, "FireworkTrail");
    out.push(u8::from(trail));
    named(out, 1, "FireworkFlicker");
    out.push(u8::from(trail));
    out.push(0);
}

/// A stack of `identifier` whose user data holds the given root entries.
fn stack(identifier: &str, entries: impl FnOnce(&mut Vec<u8>)) -> NetworkItemStack {
    let mut extra_data = vec![0xff, 0xff, 1, 10, 0, 0];
    entries(&mut extra_data);
    extra_data.push(0);
    NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == identifier)
            .unwrap()
            .network_id,
        count: 1,
        extra_data: extra_data.into(),
        ..NetworkItemStack::default()
    }
}

fn runtime() -> UiRuntime {
    runtime_with_lang("")
}

/// A runtime whose server lang holds the test names followed by `extra` lines.
fn runtime_with_lang(extra: &str) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    let names = "item.fireworks.name=Firework Rocket\n\
        item.fireworks.flight=Flight Duration:\n\
        item.fireworksCharge.name=Firework Star\n\
        item.fireworksCharge.type=Unknown shape explosion\n\
        item.fireworksCharge.type.0=Small ball explosion\n\
        item.fireworksCharge.type.4=Burst explosion\n\
        item.fireworksCharge.lightBlue=Light Blue\n\
        item.fireworksCharge.magenta=Magenta\n\
        item.fireworksCharge.red=Red\n\
        item.fireworksCharge.white=White\n\
        item.fireworksCharge.customColor=Custom\n\
        item.fireworksCharge.fadeTo=Fade to\n\
        item.fireworksCharge.trail=Trail\n\
        item.fireworksCharge.flicker=Twinkle\n";
    let input = format!("{names}{extra}");
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input.as_bytes());
        true
    }));
    runtime
}

fn texts(stack: &NetworkItemStack, identifier: &str) -> Vec<(String, [u8; 4])> {
    tooltip_lines(&runtime(), stack, Some(identifier), None)
        .into_iter()
        .skip(1)
        .map(|line| (line.text, line.color))
        .collect()
}

#[test]
fn rocket_tooltip_lists_flight_then_each_explosion() {
    let identifier = assets::facts::FIREWORK_ROCKET_IDENTIFIER;
    let rocket = stack(identifier, |out| {
        named(out, 10, "Fireworks");
        named(out, 9, "Explosions");
        out.push(10);
        out.extend(2_i32.to_le_bytes());
        explosion(out, 0, &[12], &[], false);
        explosion(out, 9, &[1, 200], &[15], true);
        named(out, 1, "Flight");
        out.push(1);
        out.push(0);
    });
    let blue = BLUE_TEXT_COLOR;
    assert_eq!(
        texts(&rocket, identifier),
        [
            ("Flight Duration: 1".to_owned(), blue),
            ("Small ball explosion".to_owned(), blue),
            ("Light Blue".to_owned(), blue),
            ("Unknown shape explosion".to_owned(), blue),
            ("Red, Custom".to_owned(), blue),
            ("Fade to White".to_owned(), blue),
            ("Trail".to_owned(), blue),
            ("Twinkle".to_owned(), blue),
        ]
    );
}

#[test]
fn star_tooltip_describes_its_explosion() {
    let identifier = assets::facts::FIREWORK_STAR_IDENTIFIER;
    let star = stack(identifier, |out| {
        named(out, 10, "FireworksItem");
        explosion(out, 0, &[13], &[], false);
    });
    assert_eq!(
        texts(&star, identifier),
        [
            ("Small ball explosion".to_owned(), BLUE_TEXT_COLOR),
            ("Magenta".to_owned(), BLUE_TEXT_COLOR),
        ]
    );
}

#[test]
fn plain_rockets_and_other_items_add_no_firework_lines() {
    let identifier = assets::facts::FIREWORK_ROCKET_IDENTIFIER;
    assert!(texts(&stack(identifier, |_| {}), identifier).is_empty());
    let star_data = stack(assets::facts::FIREWORK_STAR_IDENTIFIER, |out| {
        named(out, 10, "FireworksItem");
        explosion(out, 0, &[13], &[], false);
    });
    let apple = NetworkItemStack {
        network_id: stack("minecraft:apple", |_| {}).network_id,
        ..star_data
    };
    assert!(texts(&apple, "minecraft:apple").is_empty());
}

#[test]
fn long_colour_lists_stop_after_the_bytes_the_label_shows() {
    let identifier = assets::facts::FIREWORK_STAR_IDENTIFIER;
    let name = "R".repeat(4096);
    let runtime = runtime_with_lang(&format!("item.fireworksCharge.red={name}\n"));
    let star = stack(identifier, |out| {
        named(out, 10, "FireworksItem");
        explosion(out, 0, &[1; 20_000], &[], false);
    });
    let lines = tooltip_lines(&runtime, &star, Some(identifier), None);
    let colors = &lines[2].text;
    let shown = ui::UiLimits::MAX_TEXT_BYTES;
    assert!(
        colors.len() < shown + name.len() + 2,
        "{} bytes built",
        colors.len()
    );
    let unbounded = vec![name.as_str(); 20_000].join(", ");
    assert_eq!(colors[..shown], unbounded[..shown]);
}
