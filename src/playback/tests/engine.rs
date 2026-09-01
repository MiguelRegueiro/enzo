use super::{VOLUME_STEP_PERCENT, adjusted_volume};

#[test]
fn volume_step_is_two_percent() {
    assert_eq!(VOLUME_STEP_PERCENT, 2);
}

#[test]
fn volume_adjustment_uses_two_percent_steps_and_clamps() {
    assert_eq!(adjusted_volume(50, 1, 100), 52);
    assert_eq!(adjusted_volume(50, -2, 100), 46);
    assert_eq!(adjusted_volume(100, 1, 100), 100);
    assert_eq!(adjusted_volume(100, 1, 200), 102);
    assert_eq!(adjusted_volume(198, 2, 200), 200);
    assert_eq!(adjusted_volume(0, -1, 200), 0);
    assert_eq!(adjusted_volume(50, i32::MAX, 200), 200);
    assert_eq!(adjusted_volume(50, i32::MIN, 200), 0);
}
