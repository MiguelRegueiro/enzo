use super::*;

#[test]
fn help_uses_one_column_on_narrow_canvases() {
    let geometry = help_geometry(240, 180, 12, 14, 1);

    assert_eq!(geometry.column_count, 1);
    assert!(help_scroll_limit(240, 180, 100, None) > 0);
}

#[test]
fn help_keeps_one_column_at_compact_phone_widths() {
    let geometry = help_geometry(400, 1000, 12, 14, 1);

    assert_eq!(geometry.column_count, 1);
    assert!(geometry.panel.width as u32 >= 240);
}

#[test]
fn help_uses_three_columns_when_space_allows() {
    let geometry = help_geometry(1280, 720, 18, 22, 2);

    assert_eq!(geometry.column_count, 3);
    assert!(
        geometry.panel.width as u32 <= 750,
        "panel width was {}",
        geometry.panel.width as u32
    );
    assert!(geometry.column_gap <= 12);
    assert!(geometry.column_widths[0] >= 260);
    assert!(geometry.column_widths[1] >= 200);
    assert!(geometry.column_widths[2] >= 150);
    assert!(geometry.panel.width as u32 <= 1000);
    assert_eq!(help_scroll_limit(1280, 720, 100, None), 0);
}

#[test]
fn help_uses_three_columns_before_fullscreen_sizes() {
    let geometry = help_geometry(800, 720, 18, 22, 2);

    assert_eq!(geometry.column_count, 3);
    assert!(
        geometry.panel.width as u32 <= 750,
        "panel width was {}",
        geometry.panel.width as u32
    );
    assert!(geometry.column_widths[0] >= 260);
    assert!(geometry.column_widths[1] >= 200);
    assert!(geometry.column_widths[2] >= 150);
    assert_eq!(help_scroll_limit(800, 720, 100, None), 0);
}

#[test]
fn help_uses_two_columns_when_their_content_fits() {
    let geometry = help_geometry(640, 360, 18, 22, 2);

    assert_eq!(geometry.column_count, 2);
    assert!(geometry.column_widths[0] >= 260);
    assert!(geometry.column_widths[1] >= 200);
    assert!(help_scroll_limit(640, 360, 100, None) > 0);
}

#[test]
fn help_uses_two_columns_on_medium_widths() {
    let geometry = help_geometry(560, 1000, 18, 22, 2);

    assert_eq!(geometry.column_count, 2);
}

#[test]
fn help_widens_one_column_before_two_columns_fit() {
    let geometry = help_geometry(440, 1000, 12, 14, 1);

    assert_eq!(geometry.column_count, 1);
    assert!(geometry.panel.width as u32 >= 270);
}

#[test]
fn help_uses_three_columns_on_wide_canvases() {
    let geometry = help_geometry(760, 1000, 18, 22, 2);

    assert_eq!(geometry.column_count, 3);
}

#[test]
fn help_single_column_stays_content_sized_on_small_canvases() {
    let geometry = help_geometry(360, 220, 12, 14, 1);

    assert_eq!(geometry.column_count, 1);
    assert!(geometry.panel.width as u32 <= 170);
    assert!(geometry.column_widths[0] >= 130);
}

#[test]
fn help_does_not_sprawl_on_fullscreen_canvases() {
    let geometry = help_geometry(1920, 1080, 18, 22, 2);

    assert_eq!(geometry.column_count, 3);
    assert!(geometry.panel.width as u32 <= 1000);
}

#[test]
fn help_columns_start_with_aligned_section_titles() {
    let columns = help_columns(3);

    assert!(matches!(
        columns[0].first(),
        Some(HelpLine::Section("Playback"))
    ));
    assert!(matches!(
        columns[1].first(),
        Some(HelpLine::Section("Audio"))
    ));
    assert!(matches!(
        columns[2].first(),
        Some(HelpLine::Section("General"))
    ));
}

#[test]
fn help_general_lives_in_third_column_when_available() {
    let columns = help_columns(3);

    assert!(
        columns[2]
            .iter()
            .any(|line| matches!(line, HelpLine::Section("General")))
    );
    assert!(
        !columns[0]
            .iter()
            .any(|line| matches!(line, HelpLine::Section("General")))
    );
    assert!(
        !columns[1]
            .iter()
            .any(|line| matches!(line, HelpLine::Section("General")))
    );
}

#[test]
fn help_scrollbar_matches_picker_weight() {
    let geometry = help_geometry(240, 180, 12, 14, 1);

    assert_eq!(geometry.scrollbar_width, 3);
}

#[test]
fn help_model_keeps_case_sensitive_bindings() {
    let general = HELP_SECTIONS
        .iter()
        .find(|section| section.title == "General")
        .expect("general section");

    assert!(general.rows.iter().any(|row| row.key == "i"));
    assert!(general.rows.iter().any(|row| row.key == "I"));
}
