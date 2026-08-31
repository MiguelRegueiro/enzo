use super::*;
use crate::subtitle::srt::parse_srt;

#[test]
fn renderer_draws_active_subtitle() {
    let track = SubtitleTrack::from_cues(
        parse_srt(
            "\
1
00:00:00,000 --> 00:00:10,000
Hello
",
        )
        .expect("srt should parse"),
        None,
        String::from("Subtitles"),
    );
    let mut renderer = SubtitleRenderer::without_font();
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];

    let layout = SubtitleLayout {
        canvas_width: width,
        canvas_height: height,
        video_x: 0,
        video_y: 0,
        video_width: width,
        video_height: height,
    };
    for _ in 0..100 {
        renderer.render(&mut frame, layout, &track, Duration::from_secs(1), 0);
        if frame.iter().any(|&value| value > 220) {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    assert!(frame.iter().any(|&value| value > 220));
}

#[test]
fn renderer_prefetches_a_cue_before_its_first_active_frame() {
    let track = SubtitleTrack::from_cues(
        parse_srt(
            "\
1
00:00:01,000 --> 00:00:02,000
Ready before display
",
        )
        .expect("srt should parse"),
        None,
        String::from("Subtitles"),
    );
    let mut renderer = SubtitleRenderer::without_font();
    let width = 320;
    let height = 180;
    let layout = SubtitleLayout {
        canvas_width: width,
        canvas_height: height,
        video_x: 0,
        video_y: 0,
        video_width: width,
        video_height: height,
    };
    let mut frame = vec![20_u8; (width * height * 3) as usize];

    for _ in 0..100 {
        renderer.render(&mut frame, layout, &track, Duration::ZERO, 0);
        if !renderer.ready_overlays.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!renderer.ready_overlays.is_empty());

    frame.fill(20);
    renderer.render(&mut frame, layout, &track, Duration::from_secs(1), 0);

    assert!(frame.iter().any(|&value| value > 220));
}

#[test]
fn renderer_submits_only_when_text_or_geometry_changes() {
    let track = SubtitleTrack::from_cues(
        parse_srt(
            "\
1
00:00:00,000 --> 00:00:10,000
This subtitle is prepared once
",
        )
        .expect("srt should parse"),
        None,
        String::from("Subtitles"),
    );
    let mut renderer = SubtitleRenderer::without_font();
    let position = Duration::from_secs(1);
    let small = SubtitleLayout {
        canvas_width: 320,
        canvas_height: 180,
        video_x: 0,
        video_y: 0,
        video_width: 320,
        video_height: 180,
    };
    let mut frame = vec![0_u8; 320 * 180 * 3];

    renderer.render(&mut frame, small, &track, position, 0);
    renderer.render(&mut frame, small, &track, position, 0);
    assert_eq!(renderer.request_submissions, 1);

    renderer.render(&mut frame, small, &track, position, 28);
    for _ in 0..100 {
        if renderer.request_submissions == 2 {
            break;
        }
        thread::yield_now();
        renderer.render(&mut frame, small, &track, position, 28);
    }
    assert_eq!(renderer.request_submissions, 2);

    let large = SubtitleLayout {
        canvas_width: 640,
        canvas_height: 360,
        video_width: 640,
        video_height: 360,
        ..small
    };
    frame.resize(640 * 360 * 3, 0);
    renderer.render(&mut frame, large, &track, position, 0);
    for _ in 0..100 {
        if renderer.request_submissions == 3 {
            break;
        }
        thread::yield_now();
        renderer.render(&mut frame, large, &track, position, 0);
    }
    assert_eq!(renderer.request_submissions, 3);
}

#[test]
fn renderer_request_path_never_waits_for_worker_lock() {
    let track = SubtitleTrack::from_cues(
        parse_srt("1\n00:00:00,000 --> 00:00:10,000\nNonblocking\n").expect("srt should parse"),
        None,
        String::from("Subtitles"),
    );
    let mut renderer = SubtitleRenderer::without_font();
    let worker = Arc::clone(&renderer.worker);
    let _worker_lock = worker.data.lock().expect("worker state lock");
    let layout = SubtitleLayout {
        canvas_width: 320,
        canvas_height: 180,
        video_x: 0,
        video_y: 0,
        video_width: 320,
        video_height: 180,
    };
    let mut frame = vec![0_u8; 320 * 180 * 3];

    renderer.render(&mut frame, layout, &track, Duration::ZERO, 0);

    assert_eq!(renderer.request_submissions, 0);
    assert!(renderer.cached_overlay.is_none());
}

#[test]
fn renderer_ignores_stale_worker_results() {
    let mut renderer = SubtitleRenderer::without_font();
    let old_key = TextOverlayRequestKey {
        lines: vec![String::from("old")],
        canvas_width: 8,
        canvas_height: 8,
        bottom_reserve: 0,
    };
    let current_key = TextOverlayRequestKey {
        lines: vec![String::from("current")],
        ..old_key.clone()
    };
    renderer.set_current_text_request(Some(current_key));
    let stale_overlay = CachedTextOverlay {
        canvas_width: 8,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        premultiplied_rgba: vec![255, 255, 255, 0],
    };
    renderer
        .worker
        .data
        .lock()
        .expect("worker state lock")
        .results
        .push_back(TextOverlayResult {
            key: old_key,
            overlay: Some(stale_overlay),
        });

    renderer.poll_and_submit(false, Vec::new());

    assert!(renderer.cached_overlay.is_none());
    assert_eq!(renderer.ready_overlays.len(), 1);
    assert_eq!(renderer.request_submissions, 1);
}

#[test]
fn renderer_reports_when_the_current_overlay_becomes_ready() {
    let mut renderer = SubtitleRenderer::without_font();
    let key = TextOverlayRequestKey {
        lines: vec![String::from("current")],
        canvas_width: 8,
        canvas_height: 8,
        bottom_reserve: 0,
    };
    renderer.set_current_text_request(Some(key.clone()));
    renderer
        .worker
        .data
        .lock()
        .expect("worker state lock")
        .results
        .push_back(TextOverlayResult {
            key,
            overlay: Some(CachedTextOverlay {
                canvas_width: 8,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                premultiplied_rgba: vec![255, 255, 255, 0],
            }),
        });

    assert!(renderer.poll_ready());
    assert!(renderer.cached_overlay.is_some());
    assert!(!renderer.poll_ready());
}

#[test]
fn renderer_coalesces_pending_requests_to_the_latest_cue() {
    let mut renderer = SubtitleRenderer::without_font();
    let worker = Arc::clone(&renderer.worker);
    let mut data = worker.data.lock().expect("worker state lock");
    for index in 0..10 {
        renderer.set_current_text_request(Some(TextOverlayRequestKey {
            lines: vec![format!("cue {index}")],
            canvas_width: 320,
            canvas_height: 180,
            bottom_reserve: 0,
        }));
        data.pending = Some(TextOverlayRequest {
            key: renderer.current_key.clone().expect("current request key"),
        });
    }

    let pending = data.pending.as_ref().expect("latest request should remain");
    assert_eq!(pending.key.lines, ["cue 9"]);
}
