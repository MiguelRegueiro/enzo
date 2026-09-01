use super::*;

fn labels(count: usize) -> Vec<Arc<str>> {
    (1..=count)
        .map(|index| Arc::<str>::from(format!("Episode {index}.mkv")))
        .collect()
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
fn playlist_panel_is_centered_and_caps_visible_rows() {
    let labels = labels(30);
    let geometry = playlist_geometry(1280, 720, 18, 2, 22, &labels, None);

    assert_eq!(
        geometry.panel.left,
        1280_u32.saturating_sub(geometry.panel.right - geometry.panel.left) / 2
    );
    assert_eq!(
        geometry.panel.top,
        720_u32.saturating_sub(geometry.panel.bottom - geometry.panel.top) / 2
    );
    assert_eq!(geometry.visible_count, MAX_VISIBLE_ROWS);
    assert!(geometry.panel.right - geometry.panel.left <= 18 * 42);
}

#[test]
fn playlist_rows_map_to_scrolled_entry_indices() {
    let labels = labels(30);
    let geometry = playlist_geometry(640, 360, 18, 2, 22, &labels, None);
    let row = playlist_row_rect(geometry, 1);

    assert_eq!(
        playlist_row_at_point(geometry, point(row.left + 10, row.top + 4), 8, labels.len()),
        Some(9)
    );
    assert_eq!(
        playlist_menu_action(640, 360, 100, point(0, 0), 0, &labels, None,),
        Some(PlaylistMenuAction::Close)
    );
}
