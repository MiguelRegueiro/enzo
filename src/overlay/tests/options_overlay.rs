use super::*;

fn context(width: u32, height: u32) -> OverlayRenderContext {
    OverlayRenderContext {
        width,
        height,
        terminal_cols: 80,
        terminal_rows: 24,
        scale_percent: 100,
    }
}

fn state() -> OptionsMenuState {
    OptionsMenuState {
        name: "Blue",
        position: 2,
        count: 7,
        color: [68, 80, 239],
        editor: None,
        error: None,
    }
}

fn point(x: u32, y: u32) -> OverlayHitPoint {
    OverlayHitPoint {
        x,
        y,
        cell: HitboxRect {
            left: x,
            top: y,
            right: x,
            bottom: y,
        },
    }
}

#[test]
fn only_arrow_buttons_cycle_presets() {
    let context = context(1280, 720);
    let state = state();
    let geometry = geometry(context, &state, &mut None);
    for direction in [-1, 1] {
        let button = geometry.arrow(direction);
        assert_eq!(
            options_action(
                context,
                &state,
                point(button.left + 1, button.top + 1),
                None
            ),
            Some(OptionsAction::Cycle(direction))
        );
    }
    let row = geometry.row(2);
    assert_eq!(
        options_action(
            context,
            &state,
            point((row.left + row.right) / 2, row.top + 1),
            None
        ),
        None
    );
    assert_eq!(
        options_action(
            context,
            &state,
            point(geometry.arrow(-1).right + 1, row.top + 1),
            None
        ),
        None
    );
    assert_eq!(
        options_action(context, &state, point(0, 0), None),
        Some(OptionsAction::Close)
    );
}

#[test]
fn clicking_hex_field_places_caret_at_measured_character() {
    let context = context(1280, 720);
    let mut state = state();
    state.name = "Custom";
    state.editor = Some(HexInputState {
        text: "#123456".into(),
        cursor: 7,
        selected: true,
        focused: true,
    });
    let geometry = geometry(context, &state, &mut None);
    let row = geometry.editor_rect(state.name, &mut None);
    let x = row.left + overlay_text_width(&mut None, "#12", geometry.scale);
    assert_eq!(
        options_action(context, &state, point(x, row.top + 1), None),
        Some(OptionsAction::Cursor(3))
    );
}

#[test]
fn panel_sizes_to_content_and_stays_bounded_on_small_canvases() {
    for (width, height) in [(1, 1), (120, 80), (320, 180), (1920, 1080)] {
        let context = context(width, height);
        let state = state();
        let geometry = geometry(context, &state, &mut None);
        assert!(geometry.panel.right <= width);
        assert!(geometry.panel.bottom <= height);
        let mut frame = vec![20; width as usize * height as usize * 3];
        draw_options_menu(
            None,
            &mut frame,
            context,
            &state,
            &mut AcrylicScratch::default(),
        );
        if height >= 180 {
            assert_eq!(
                geometry.panel.bottom - geometry.panel.top,
                geometry.pitch * 3 + geometry.pad * 2
            );
        }
    }
}
