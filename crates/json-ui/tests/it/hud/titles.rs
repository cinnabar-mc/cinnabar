use super::{Draw, HudModel, HudTitle, render_with};

fn titled(title: &str, subtitle: &str) -> HudModel {
    HudModel {
        title: Some(HudTitle {
            creation_id: 1,
            title: title.into(),
            subtitle: subtitle.into(),
            fade_in: 0.5,
            stay: 3.5,
            fade_out: 1.0,
            background_alpha: 0.0,
            born: 0.0,
        }),
        ..HudModel::default()
    }
}

/// Top of the drawn label showing `text`, in the vanilla layout or the built-in pack's.
fn label_top(model: &HudModel, java: bool, text: &str) -> Option<f64> {
    let nodes = render_with(model, java)?;
    nodes.iter().find_map(|node| {
        matches!(&node.draw, Draw::Text { text: drawn, .. } if drawn == text).then_some(node.dest.y)
    })
}

/// A server that pads a subtitle with blank lines relies on the title block being a centred
/// stack: every added line lifts the text by half a line. The built-in pack lifts it as far as
/// vanilla does, while an ordinary one-line title keeps its fixed placement.
#[test]
fn subtitle_blank_lines_lift_the_built_in_title_like_vanilla() {
    let one_line = titled(" ", "Victory!");
    let padded_text = format!("Victory!{}", "\n".repeat(10));
    let padded = titled(" ", &padded_text);
    let lift = |java| {
        Some(label_top(&one_line, java, "Victory!")? - label_top(&padded, java, &padded_text)?)
    };
    let Some(vanilla) = lift(false) else {
        return;
    };
    let built_in = lift(true).expect("built-in layout draws both titles");
    assert!(vanilla > 0.0, "vanilla lifts the padded subtitle");
    assert!(
        (built_in - vanilla).abs() <= 1.0,
        "lifted {built_in} against vanilla's {vanilla}"
    );
}

#[test]
fn one_line_title_keeps_the_java_placement() {
    let model = titled("Title", "Sub");
    let Some(title) = label_top(&model, true, "Title") else {
        return;
    };
    assert_eq!(title, 95.0);
    assert_eq!(label_top(&model, true, "Sub"), Some(145.0));
}
