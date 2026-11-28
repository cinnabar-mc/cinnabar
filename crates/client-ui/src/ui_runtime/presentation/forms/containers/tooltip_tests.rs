use super::tooltip_text;
use crate::ui_runtime::presentation::hud_layout::TooltipLine;
use protocol::NetworkItemStack;
use sha2::{Digest, Sha256};

/// Resolves a server-authored rawtext lore through the real inventory tooltip and
/// serializes the final `#hover_text`: the second section must keep the first one's
/// green/bold formatting, and `§r` must end it on the third line.
#[test]
fn resolved_lore_keeps_section_formatting_in_final_tooltip_text() {
    let lore = serde_json::json!({"rawtext": [{"text": "§a§lFirst\nSecond\n§rThird"}]}).to_string();
    let mut extra = (-1_i16).to_le_bytes().to_vec();
    extra.extend([1, 10, 0, 0]);
    extra.extend([10, 7, 0]);
    extra.extend(b"display");
    extra.extend([9, 4, 0]);
    extra.extend(b"Lore");
    extra.push(8);
    extra.extend(1_i32.to_le_bytes());
    extra.extend(u16::try_from(lore.len()).unwrap().to_le_bytes());
    extra.extend(lore.as_bytes());
    extra.extend([0, 0]);
    extra.extend([0; 8]);
    let stack = NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == "minecraft:diamond_sword")
            .unwrap()
            .network_id,
        count: 1,
        stack_network_id: 1,
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..NetworkItemStack::empty()
    };
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    let lines = crate::ui_runtime::presentation::inventory_tooltip::tooltip_lines(
        &runtime,
        &stack,
        Some("minecraft:diamond_sword"),
        None,
    );
    assert_eq!(lines.len(), 4, "name plus three resolved lore sections");
    let encoded = tooltip_text(&lines).expect("tooltip has content");
    assert!(encoded.contains("First"), "{encoded}");
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    let styled = |needle: &str| {
        parsed
            .iter()
            .find(|span| span.text.contains(needle))
            .unwrap_or_else(|| panic!("{needle} is present in {encoded}"))
            .style
    };
    assert_eq!(styled("First").color, ui::BedrockColor::Green);
    assert!(styled("First").bold);
    assert_eq!(
        styled("Second").color,
        ui::BedrockColor::Green,
        "§a carries into the next section: {encoded}"
    );
    assert!(
        styled("Second").bold,
        "§l carries into the next section: {encoded}"
    );
    assert!(
        !styled("Third").bold && styled("Third").color != ui::BedrockColor::Green,
        "§r ends the carry: {encoded}"
    );
}

#[test]
fn tooltip_serializes_every_shared_palette_color_not_only_enchantments_and_lore() {
    for code in ('0'..='9').chain('a'..='v') {
        if matches!(code, 'k' | 'l' | 'o' | 'r') {
            continue;
        }
        let source = format!("§{code}Name");
        let parsed = ui::parse_bedrock_text(&source, source.len()).unwrap();
        let color = parsed[0].style.color;
        let [r, g, b] = color.rgb().unwrap_or([255; 3]);
        let encoded = tooltip_text(&[TooltipLine {
            text: "Name".into(),
            color: [r, g, b, 255],
        }])
        .unwrap();
        let decoded = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
        assert_eq!(decoded[0].style.color, color, "{code}: {encoded}");
        assert_eq!(decoded.plain_text(), "Name");
    }
}

#[test]
fn independent_line_styles_do_not_inherit_a_custom_names_bold_or_italic() {
    let gray = ui::BedrockColor::Gray.rgb().unwrap();
    let lines = [
        TooltipLine {
            text: "§l§oName".into(),
            color: [255; 4],
        },
        TooltipLine {
            text: "Enchantment".into(),
            color: [gray[0], gray[1], gray[2], 255],
        },
    ];
    let encoded = tooltip_text(&lines).unwrap();
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    let enchant = parsed
        .iter()
        .find(|span| span.text.contains("Enchantment"))
        .unwrap();
    assert_eq!(enchant.style.color, ui::BedrockColor::Gray);
    assert!(!enchant.style.bold);
    assert!(!enchant.style.italic);
}

#[test]
fn server_authored_format_codes_still_override_the_line_base_color() {
    let aqua = ui::BedrockColor::Aqua.rgb().unwrap();
    let encoded = tooltip_text(&[TooltipLine {
        text: "§cServer Name".into(),
        color: [aqua[0], aqua[1], aqua[2], 255],
    }])
    .unwrap();
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    assert_eq!(parsed[0].style.color, ui::BedrockColor::Red);
    assert!(tooltip_text(&[]).is_none());
}

#[test]
fn component_native_code_survives_when_legacy_rgb_does_not_match_the_ui_palette() {
    let encoded = tooltip_text(&[TooltipLine {
        text: "§vName".into(),
        color: [1, 2, 3, 255],
    }])
    .unwrap();
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    assert_eq!(parsed[0].style.color, ui::BedrockColor::MaterialResin);
}
