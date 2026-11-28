//! Splitting already-resolved display text into tooltip lines.

/// Collects the format codes still active at the end of a resolved text line.
fn formatting_codes(text: &str) -> String {
    let mut active = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '§' {
            continue;
        }
        let Some(code) = chars.next() else {
            break;
        };
        if code == 'r' {
            active.clear();
        } else if !active.contains(code) {
            active.push('§');
            active.push(code);
        }
    }
    active
}

/// Splits resolved text at newlines, carrying active `§` codes into each following line.
#[must_use]
pub fn lore_lines(text: &str) -> Vec<String> {
    let mut carried = String::new();
    text.split('\n')
        .map(|segment| {
            let mut line = String::with_capacity(carried.len() + segment.len());
            line.push_str(&carried);
            line.push_str(segment);
            carried = formatting_codes(&line);
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::lore_lines;

    #[test]
    fn lore_split_carries_active_formatting_and_resets() {
        let cases = [
            ("active code carries", "§aFirst\nSecond", &["§aFirst", "§aSecond"][..]),
            ("reset clears carry", "§aFirst\n§rSecond", &["§aFirst", "§a§rSecond"][..]),
            ("plain text is unchanged", "plain\nlines", &["plain", "lines"][..]),
            (
                "multiple codes carry in order",
                "§a§lFirst\nSecond",
                &["§a§lFirst", "§a§lSecond"][..],
            ),
            (
                "reset before the break carries nothing",
                "§aFirst§r\nSecond",
                &["§aFirst§r", "Second"][..],
            ),
        ];
        for (name, input, expected) in cases {
            assert_eq!(lore_lines(input), expected, "{name}");
        }
    }
}
