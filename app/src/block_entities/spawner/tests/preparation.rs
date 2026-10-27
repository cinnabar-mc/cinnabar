//! Candidate scratch preserves stable visibility priority while reusing storage.

use super::*;

#[test]
fn warmed_candidate_preparation_reuses_storage_and_preserves_scan_order() {
    let requests: Vec<Candidate> = (0..512)
        .map(|index| (ChunkKey::new(1, index as i32, 0), index))
        .collect();
    let expected: Vec<_> = requests
        .iter()
        .copied()
        .filter(|&(_, index)| index % 7 != 0 && index % 3 == 0)
        .chain(
            requests
                .iter()
                .copied()
                .filter(|&(_, index)| index % 7 != 0 && index % 3 != 0),
        )
        .collect();
    let mut scratch = PreparationScratch::default();
    for _ in 0..3 {
        scratch.prepare_candidates(
            requests.iter().copied(),
            |(_, index)| index % 7 != 0,
            |(_, index)| index % 3 == 0,
        );
    }
    let mut viewed = 0;
    let (_, allocations) = test_alloc::measure(|| {
        scratch.prepare_candidates(
            requests.iter().copied(),
            |(_, index)| index % 7 != 0,
            |(_, index)| {
                viewed += 1;
                index % 3 == 0
            },
        );
    });
    assert_eq!(allocations, test_alloc::Allocations::default());
    assert_eq!(scratch.candidates, expected);
    assert_eq!(
        viewed,
        requests.iter().filter(|(_, index)| index % 7 != 0).count()
    );
    scratch.prepare_candidates(
        requests.iter().copied(),
        |(_, index)| index % 2 == 0,
        |(_, index)| index % 5 == 0,
    );
    let changed: Vec<_> = requests
        .iter()
        .copied()
        .filter(|&(_, index)| index % 2 == 0 && index % 5 == 0)
        .chain(
            requests
                .iter()
                .copied()
                .filter(|&(_, index)| index % 2 == 0 && index % 5 != 0),
        )
        .collect();
    assert_eq!(scratch.candidates, changed);
    scratch.prepare_candidates(requests.iter().copied(), |_| false, |_| unreachable!());
    assert!(scratch.candidates.is_empty());
}
