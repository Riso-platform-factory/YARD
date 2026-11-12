use super::*;

fn resized(sizes: &[f32], handle: usize, change: f32, minimum: f32) -> Vec<f32> {
    let length = sizes.iter().sum::<f32>();
    let mut shares: Vec<_> = sizes
        .iter()
        .map(|size| size * sizes.len() as f32 / length)
        .collect();
    compute_resize(&mut shares, handle, length, sizes[handle] + change, minimum);
    shares
        .iter()
        .map(|share| length * share / sizes.len() as f32)
        .collect()
}

fn assert_sizes(actual: &[f32], expected: &[f32]) {
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
    }
}

#[test]
fn plain_drag() {
    assert_sizes(
        &resized(&[200., 200., 200.], 0, 40., 80.),
        &[240., 160., 200.],
    );
}
#[test]
fn clamps_at_minimum() {
    assert_sizes(&resized(&[200., 200.], 0, 300., 80.), &[320., 80.]);
}
#[test]
fn pushes_successors() {
    assert_sizes(
        &resized(&[200., 200., 200.], 0, 200., 80.),
        &[400., 80., 120.],
    );
}
#[test]
fn backwards_drag_pushes_predecessors() {
    assert_sizes(
        &resized(&[200., 200., 200.], 1, -200., 80.),
        &[120., 80., 400.],
    );
}
#[test]
fn too_small_returns_early() {
    assert_sizes(
        &resized(&[70., 200., 200.], 0, 20., 80.),
        &[70., 200., 200.],
    );
    assert_sizes(
        &resized(&[200., 70., 200.], 0, 20., 80.),
        &[200., 70., 200.],
    );
}
#[test]
fn shares_sum_stays_the_count() {
    for handle in 0..3 {
        let mut shares = vec![1.; 4];
        for change in [-900., -90., 0., 25., 900.] {
            compute_resize(&mut shares, handle, 800., 200. + change, 80.);
            assert!((shares.iter().sum::<f32>() - 4.).abs() < 0.001);
            assert!(shares.iter().all(|share| 800. * share / 4. >= 79.999));
        }
    }
}
