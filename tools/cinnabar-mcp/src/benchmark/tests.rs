use super::*;

/// Builds an original straight path whose duration is twelve seconds at sixteen blocks per second.
fn route() -> Route {
    Route {
        name: "flight".into(),
        start: [0., 80., 0.],
        end: [192., 80., 0.],
        yaw: -90.,
        pitch: 20.,
        vertical_fov: 70.,
        speed: 16.,
    }
}

/// Makes a loss-free trace with a stall that ends beyond the intended flight window.
fn trace_fixture() -> Value {
    let mut events = vec![json!({"name": "benchmark_path_start", "ts": 0, "ph": "X", "dur": 0})];
    for (i, seconds) in [0., 2., 3., 16., 17.].into_iter().enumerate() {
        events
            .push(json!({"name": "frame_start", "ts": seconds * 1e6, "frame_id": i+1, "ph": "i"}));
    }
    events
        .push(json!({"name": "main_frame", "ts": 2100000, "dur": 12000, "frame_id": 2, "ph": "X"}));
    events.push(json!({"name": "gpu_frame", "ts": 3100000, "cat": "gpu_readback", "ph": "C", "frame_id": null,
        "args": {"duration_ns": 1000000}}));
    json!({"truncated": false, "dropped_events": 0, "overwritten_events": 0, "traceEvents": events})
}

#[test]
fn measured_frames_keep_the_successor_interval_after_the_route_deadline() {
    let result = trace::reduce(&trace_fixture(), &[route()], 2.).unwrap();
    assert_eq!(result.frames[0].frames, 2);
    assert_eq!(result.frames[0].seconds, 14.);
    assert_eq!(result.frames[0].max_ms, 13000.);
    assert_eq!(result.phases[0]["overrun_seconds"], 2.);
    assert_eq!(
        result.phases[0]["cpu_stage_spans"]["main_frame"]["mean_ms"],
        12.
    );
    assert_eq!(result.phases[0]["gpu_origin_ids"], "unavailable");
    assert_eq!(
        result.phases[0]["gpu_received_samples"]["gpu_frame"]["samples"],
        1
    );
}

#[test]
fn lost_frames_overwritten_traces_and_unclosed_intervals_are_rejected() {
    let mut trace = trace_fixture();
    trace["dropped_events"] = json!(1);
    assert!(trace::reduce(&trace, &[route()], 2.).is_err());
    trace = trace_fixture();
    trace["traceEvents"][3]["frame_id"] = json!(99);
    assert!(trace::reduce(&trace, &[route()], 2.).is_err());
    trace = trace_fixture();
    trace["traceEvents"]
        .as_array_mut()
        .unwrap()
        .retain(|event| event["ts"].as_f64().unwrap() < 14e6);
    assert!(trace::reduce(&trace, &[route()], 2.).is_err());
}

/// Builds the required comparison envelope without fabricating a real hardware observation.
fn comparison_fixture() -> (Value, Value) {
    let hardware = json!({"os_version": "test OS", "cpu": {"name": "test CPU"}, "gpu": {
        "name": "test GPU", "backend": "test", "driver": "test driver"}});
    let conditions = json!({"scenario": "test scene", "render_distance": 12, "window": [1920,1080],
        "vsync": false, "offscreen": true, "repeats": 1, "warmup_s": 2, "power": "ac", "backend": "test"});
    let report = json!({"conditions": conditions, "hardware": hardware,
        "frames": [{"phase": "flight", "frames": 2}]});
    let manifest = json!({"comparison_key": {
        "machine_id": "test machine", "hardware": hardware, "scene": "scene hash", "server": "server hash",
        "cache": "cache hash", "semantics": "test semantics", "instrumentation": "test instrumentation",
        "server_placement": "same_machine", "conditions": conditions, "routes": [{"name": "flight"}],
        "settings": {"camera": {"viewport": [1920,1080], "projection": "test projection"},
            "video": {"native_video_options": "test video"}}
    }});
    (report, manifest)
}

#[test]
fn comparison_rejects_unknown_conditions_changed_routes_and_missing_phases() {
    let (a, am) = comparison_fixture();
    assert!(report::compatible(&a, &am, &a, &am).is_ok());
    for pointer in [
        "/comparison_key/machine_id",
        "/comparison_key/hardware/gpu/driver",
    ] {
        let mut bad = am.clone();
        *bad.pointer_mut(pointer).unwrap() = Value::Null;
        assert!(report::compatible(&a, &bad, &a, &bad).is_err());
    }
    let mut changed = am.clone();
    changed["comparison_key"]["routes"][0]["speed"] = json!(32);
    assert!(report::compatible(&a, &am, &a, &changed).is_err());
    let mut missing = a.clone();
    missing["frames"] = json!([]);
    assert!(report::compatible(&a, &am, &missing, &am).is_err());
    let mut resized = a.clone();
    resized["conditions"]["window"] = json!([1280, 720]);
    assert!(report::compatible(&a, &am, &resized, &am).is_err());
}

#[test]
fn routes_have_constant_speed_after_settling_and_reject_bad_inputs() {
    let mut route = route();
    let path = route.path(2.).unwrap();
    assert_eq!(path.duration(), 14.);
    assert_eq!(path.sample(2.).unwrap().position[0], 0.);
    assert_eq!(path.sample(8.).unwrap().position[0], 96.);
    route.speed = 0.;
    assert!(route.path(2.).is_err());
}

#[test]
fn readiness_square_fits_the_server_delivery_circle() {
    for radius in 1..=64_u32 {
        let square = plan::resident_square_radius(radius);
        assert!(2 * square * square <= radius * radius);
        assert!(2 * (square + 1) * (square + 1) > radius * radius);
    }
    assert_eq!(plan::resident_square_radius(12), 8);
}

#[test]
fn memory_includes_descendants_without_charging_unrelated_processes() {
    assert_eq!(
        observe::family_rss("10 1 100\n11 10 200\n12 11 300\n13 1 999\n", 10).unwrap(),
        600 * 1024
    );
    assert!(observe::family_rss("11 10 200", 10).is_err());
}

#[test]
fn frozen_world_copy_is_stable_and_never_changes_the_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("fixture"), b"original fixture").unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    let hash = files::copy_tree(&source, &first).unwrap();
    assert_eq!(hash, files::copy_tree(&source, &second).unwrap());
    std::fs::write(first.join("fixture"), b"changed copy").unwrap();
    assert_eq!(
        std::fs::read(source.join("fixture")).unwrap(),
        b"original fixture"
    );
}
