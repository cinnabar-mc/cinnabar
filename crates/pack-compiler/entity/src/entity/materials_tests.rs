use super::*;
use serde_json::json;

#[test]
fn use_color_mask_follows_authored_define_inheritance() {
    let mut states = MaterialStates::default();
    states.read(&json!({"materials": {
        "xray_box:entity_alphablend": {
            "+states": ["DisableDepthWrite"],
            "depthFunc": "Always",
            "+defines": ["USE_COLOR_MASK"],
            "-defines": ["ENABLE_FOG"]
        },
        "removed:xray_box": {"-defines": ["USE_COLOR_MASK"]},
        "replaced:xray_box": {"defines": []},
        "inherited:xray_box": {}
    }}));
    let masked = states.resolve("xray_box").unwrap();
    assert!(masked.color_mask);
    assert!(masked.blend && masked.depth_always && !masked.depth_write);
    assert!(states.resolve("inherited.skinning").unwrap().color_mask);
    assert!(!states.resolve("removed").unwrap().color_mask);
    assert!(!states.resolve("replaced").unwrap().color_mask);
    assert!(!states.resolve("entity_alphablend").unwrap().color_mask);
}
