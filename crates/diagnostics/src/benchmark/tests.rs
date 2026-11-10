use super::*;

#[test]
fn a_long_stall_is_preserved_and_lows_are_tail_means() {
    let mut values = vec![1.; 999];
    values.push(100.);
    values.push(2000.);
    let result = Frames::new("flight", &values).unwrap();
    assert_eq!(result.frames, 1001);
    assert_eq!(result.max_ms, 2000.);
    assert_eq!(result.low_01pct_fps, 0.952);
    assert_eq!(result.p999_ms, 100.);
    assert_eq!(result.histogram.iter().sum::<u64>(), 1001);
    assert_eq!(result.histogram.last(), Some(&1));
    assert_eq!(result.avg_fps, 323.007);
}

#[test]
fn exact_edges_use_the_bucket_starting_at_that_edge() {
    let values: Vec<_> = HISTOGRAM_EDGES_MS.iter().copied().skip(1).collect();
    let result = Frames::new("edges", &values).unwrap();
    assert_eq!(result.histogram[0], 0);
    assert!(result.histogram[1..].iter().all(|count| *count == 1));
}

#[test]
fn tiny_samples_and_invalid_data_do_not_invent_measurements() {
    let result = Frames::new("one", &[10.]).unwrap();
    assert_eq!(result.low_01pct_fps, 100.);
    assert_eq!(result.p999_ms, 10.);
    for values in [
        vec![],
        vec![0.],
        vec![-1.],
        vec![f64::NAN],
        vec![1., f64::INFINITY],
    ] {
        assert!(Frames::new("bad", &values).is_err());
    }
    let summary = Summary::single(4.).unwrap();
    assert_eq!(summary.stdev, None);
    assert_eq!(summary.ci95, None);
    assert_eq!(Distribution::new(&[0., 2.]).unwrap().p50_ms, 0.);
}
