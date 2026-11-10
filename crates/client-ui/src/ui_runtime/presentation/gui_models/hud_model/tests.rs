use super::*;
use ui::{UiNodeId, UiPoint, UiRect};

#[test]
fn node_color_modulates_high_range_controller_tints_without_quantizing_them() {
    let tint = [1.75, 0.5, 4.0, 0.75];
    let bone = UiModelBone {
        screen: Mat4::IDENTITY.to_cols_array_2d(),
        normal: [[0.; 4]; 3],
        overlay: [0.; 4],
        uv_rect: [0., 0., 1., 1.],
        uv_transform: [0., 0., 1., 1.],
        uv_cross: [0.; 4],
        tint,
        styles: [0; 4],
    };
    let mut model = HudModel {
        palette: Arc::from([bone]),
        low: Vec3::ZERO,
        high: Vec3::ONE,
        ..Default::default()
    };
    let bounds = UiRect::new(
        UiPoint::new(0., 0.).unwrap(),
        UiPoint::new(16., 16.).unwrap(),
    )
    .unwrap();
    let node = UiNode::new(UiNodeId::new(1), None, bounds);
    let color = [255, 128, 64, 128];
    let placed = model.pose(0, &node, bounds, color, 1., [100, 100]);

    for channel in 0..4 {
        let expected = tint[channel] * (f32::from(color[channel]) / 255.);
        assert!((placed.bones[0].tint[channel] - expected).abs() < 0.000001);
    }
    assert!(placed.bones[0].tint[0] > 1.);
    assert!(placed.bones[0].tint[2] > 1.);
    assert_eq!(model.palette[0].tint, tint);
}
