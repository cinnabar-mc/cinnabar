use super::*;

#[test]
fn full_size_is_dlaa_and_reduced_sizes_pick_the_nearest_quality_mode() {
    let output = [2560, 1440];
    assert_eq!(mode_for(output, output), DlssPerfQualityMode::Dlaa);
    for (render_width, mode) in [
        (2400, DlssPerfQualityMode::Quality),
        (1707, DlssPerfQualityMode::Quality),
        (1485, DlssPerfQualityMode::Balanced),
        (1280, DlssPerfQualityMode::Performance),
        (853, DlssPerfQualityMode::UltraPerformance),
        (256, DlssPerfQualityMode::UltraPerformance),
    ] {
        let render = [render_width, render_width * 1440 / 2560];
        assert_eq!(mode_for(render, output), mode, "{render:?}");
    }
}

#[test]
fn render_sizes_outside_either_axis_of_the_range_are_refused() {
    let range = [1280, 720]..=[1920, 1080];
    assert!(admits(&range, [1280, 720]));
    assert!(admits(&range, [1600, 900]));
    assert!(!admits(&range, [1200, 900]));
    assert!(!admits(&range, [1600, 1100]));
}
