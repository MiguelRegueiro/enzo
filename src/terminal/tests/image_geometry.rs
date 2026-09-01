use super::*;

#[test]
fn image_area_preserves_source_aspect() {
    let area = fit_image_area(1920, 1080, 80, 24, 10.0, 20.0);

    assert_eq!(area.cols, 80);
    assert_eq!(area.rows, 22);
    assert_eq!(area.x, 0);
    assert_eq!(area.y, 1);
}
