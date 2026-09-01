use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicI64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;

use super::{
    video_frame_decoder::{NativeFrame, NativeVideoDecoder},
    video_frame_store::{
        LatestFrame, mark_ended, mark_error, new_frame_buffer, reset_frame_state,
        store_latest_frame, validate_frame_guard,
    },
    video_timing::{
        VIDEO_CLOCK_DROP_LAG, VIDEO_CLOCK_LEAD, master_clock_position, should_publish_late_frame,
        stale_frame_drop_before,
    },
};

#[derive(Clone)]
pub(super) struct VideoThreadState {
    pub(super) latest_frame: Arc<Mutex<LatestFrame>>,
    pub(super) stop: Arc<AtomicI32>,
    pub(super) pause: Arc<AtomicI32>,
    pub(super) seek_generation: Arc<AtomicI32>,
    pub(super) seek_micros: Arc<AtomicI64>,
    pub(super) seek_exact: Arc<AtomicI32>,
    pub(super) released_seek_generation: Arc<AtomicI32>,
    pub(super) master_clock: Arc<Mutex<Option<Arc<AtomicI64>>>>,
}

pub(super) fn run_video_decode_thread(
    mut native: NativeVideoDecoder,
    frame_len: usize,
    fps: f64,
    shared: VideoThreadState,
) {
    let VideoThreadState {
        latest_frame,
        stop,
        pause,
        seek_generation,
        seek_micros,
        seek_exact,
        released_seek_generation,
        master_clock,
    } = shared;
    let mut started_at = Instant::now();
    let fallback_interval = 1.0 / fps.max(1.0);
    let mut fallback_pts = 0.0;
    let mut buffer = match new_frame_buffer(frame_len) {
        Ok(buffer) => buffer,
        Err(error) => {
            mark_error(&latest_frame, error.to_string());
            return;
        }
    };
    let mut seen_seek_generation = 0;
    let mut force_next_frame = false;
    let mut clocked_seek_generation = 0;
    let mut last_published_pts = None::<Duration>;
    let mut last_published_at = Instant::now();
    let mut consecutive_clock_drops = 0_u32;

    loop {
        if stop.load(Ordering::Relaxed) != 0 {
            mark_ended(&latest_frame);
            break;
        }

        if let Some(request) = take_seek_request(
            &seek_generation,
            &seek_micros,
            &seek_exact,
            &mut seen_seek_generation,
        ) {
            if let Err(error) = seek_video_thread(
                &mut native,
                &latest_frame,
                request.position,
                request.exact,
                &stop,
                &mut started_at,
                &mut fallback_pts,
            ) {
                mark_error(&latest_frame, error.to_string());
                break;
            }
            force_next_frame = true;
            consecutive_clock_drops = 0;
            last_published_at = Instant::now();
        }

        if !force_next_frame {
            match wait_while_paused(
                &stop,
                &pause,
                &seek_generation,
                &seek_micros,
                &seek_exact,
                &mut seen_seek_generation,
            ) {
                PauseWait::Ready(paused_for) => {
                    if released_seek_generation.load(Ordering::Acquire) == seen_seek_generation
                        && clocked_seek_generation != seen_seek_generation
                        && let Some(pts) = last_published_pts
                    {
                        started_at = Instant::now() - pts;
                        clocked_seek_generation = seen_seek_generation;
                    } else {
                        started_at += paused_for;
                    }
                }
                PauseWait::Seek(request, paused_for) => {
                    started_at += paused_for;
                    if let Err(error) = seek_video_thread(
                        &mut native,
                        &latest_frame,
                        request.position,
                        request.exact,
                        &stop,
                        &mut started_at,
                        &mut fallback_pts,
                    ) {
                        mark_error(&latest_frame, error.to_string());
                        break;
                    }
                    force_next_frame = true;
                    consecutive_clock_drops = 0;
                    last_published_at = Instant::now();
                }
                PauseWait::Stopped => {
                    mark_ended(&latest_frame);
                    break;
                }
            }
        }

        let drop_before = (!force_next_frame)
            .then(|| stale_frame_drop_before(&master_clock))
            .flatten();
        let publish_late_frame = drop_before.is_some()
            && should_publish_late_frame(
                consecutive_clock_drops,
                last_published_at,
                Instant::now(),
            );
        let drop_before_pts = if publish_late_frame {
            f64::NAN
        } else {
            drop_before.map(|pts| pts.as_secs_f64()).unwrap_or(f64::NAN)
        };
        let pts = match native.next_frame(
            &mut buffer,
            drop_before_pts,
            &stop,
            &seek_generation,
            seen_seek_generation,
        ) {
            Ok(NativeFrame::Frame(pts)) => pts,
            Ok(NativeFrame::Dropped) => {
                consecutive_clock_drops = consecutive_clock_drops.saturating_add(1);
                continue;
            }
            Ok(NativeFrame::Interrupted) => continue,
            Ok(NativeFrame::Ended) => {
                mark_ended(&latest_frame);
                break;
            }
            Err(error) => {
                mark_error(&latest_frame, error.to_string());
                break;
            }
        };
        if let Err(error) = validate_frame_guard(&buffer, frame_len) {
            mark_error(&latest_frame, error.to_string());
            break;
        }

        let pts = if pts.is_finite() && pts >= 0.0 {
            pts
        } else {
            let pts = fallback_pts;
            fallback_pts += fallback_interval;
            pts
        };
        let pts_duration = Duration::from_secs_f64(pts);
        if !force_next_frame && !publish_late_frame {
            let mut due_at = started_at + pts_duration;
            let mut drop_frame = false;
            loop {
                if stop.load(Ordering::Relaxed) != 0 {
                    mark_ended(&latest_frame);
                    return;
                }
                match wait_while_paused(
                    &stop,
                    &pause,
                    &seek_generation,
                    &seek_micros,
                    &seek_exact,
                    &mut seen_seek_generation,
                ) {
                    PauseWait::Ready(paused_for) => {
                        started_at += paused_for;
                        due_at += paused_for;
                    }
                    PauseWait::Seek(request, paused_for) => {
                        started_at += paused_for;
                        if let Err(error) = seek_video_thread(
                            &mut native,
                            &latest_frame,
                            request.position,
                            request.exact,
                            &stop,
                            &mut started_at,
                            &mut fallback_pts,
                        ) {
                            mark_error(&latest_frame, error.to_string());
                            return;
                        }
                        force_next_frame = true;
                        break;
                    }
                    PauseWait::Stopped => {
                        mark_ended(&latest_frame);
                        return;
                    }
                }

                if let Some(master_position) = master_clock_position(&master_clock) {
                    if pts_duration.saturating_add(VIDEO_CLOCK_DROP_LAG) < master_position {
                        drop_frame = true;
                        break;
                    }
                    if pts_duration <= master_position.saturating_add(VIDEO_CLOCK_LEAD) {
                        break;
                    }
                    let wait = pts_duration
                        .saturating_sub(master_position)
                        .saturating_sub(VIDEO_CLOCK_LEAD);
                    thread::sleep(wait.min(Duration::from_millis(10)));
                    continue;
                }

                let now = Instant::now();
                if due_at <= now {
                    break;
                }
                thread::sleep((due_at - now).min(Duration::from_millis(10)));
            }
            if force_next_frame || drop_frame {
                if drop_frame {
                    consecutive_clock_drops = consecutive_clock_drops.saturating_add(1);
                }
                continue;
            }
        }

        if let Some(request) = take_seek_request(
            &seek_generation,
            &seek_micros,
            &seek_exact,
            &mut seen_seek_generation,
        ) {
            if let Err(error) = seek_video_thread(
                &mut native,
                &latest_frame,
                request.position,
                request.exact,
                &stop,
                &mut started_at,
                &mut fallback_pts,
            ) {
                mark_error(&latest_frame, error.to_string());
                break;
            }
            force_next_frame = true;
            consecutive_clock_drops = 0;
            last_published_at = Instant::now();
            continue;
        }

        buffer = store_latest_frame(
            &latest_frame,
            buffer,
            pts_duration,
            &seek_generation,
            seen_seek_generation,
        );
        last_published_pts = Some(pts_duration);
        last_published_at = Instant::now();
        consecutive_clock_drops = 0;
        force_next_frame = false;
    }
}

struct SeekRequest {
    position: Duration,
    exact: bool,
}

enum PauseWait {
    Ready(Duration),
    Seek(SeekRequest, Duration),
    Stopped,
}

fn wait_while_paused(
    stop: &AtomicI32,
    pause: &AtomicI32,
    seek_generation: &AtomicI32,
    seek_micros: &AtomicI64,
    seek_exact: &AtomicI32,
    seen_seek_generation: &mut i32,
) -> PauseWait {
    if pause.load(Ordering::Relaxed) == 0 {
        return PauseWait::Ready(Duration::ZERO);
    }

    let paused_at = Instant::now();
    while pause.load(Ordering::Relaxed) != 0 {
        if stop.load(Ordering::Relaxed) != 0 {
            return PauseWait::Stopped;
        }
        if let Some(request) = take_seek_request(
            seek_generation,
            seek_micros,
            seek_exact,
            seen_seek_generation,
        ) {
            return PauseWait::Seek(request, paused_at.elapsed());
        }
        thread::sleep(Duration::from_millis(5));
    }
    PauseWait::Ready(paused_at.elapsed())
}

fn take_seek_request(
    seek_generation: &AtomicI32,
    seek_micros: &AtomicI64,
    seek_exact: &AtomicI32,
    seen_seek_generation: &mut i32,
) -> Option<SeekRequest> {
    let generation = seek_generation.load(Ordering::Acquire);
    if generation == *seen_seek_generation {
        return None;
    }
    *seen_seek_generation = generation;
    let micros = seek_micros.load(Ordering::Relaxed).max(0) as u64;
    Some(SeekRequest {
        position: Duration::from_micros(micros),
        exact: seek_exact.load(Ordering::Acquire) != 0,
    })
}

fn seek_video_thread(
    native: &mut NativeVideoDecoder,
    latest_frame: &Arc<Mutex<LatestFrame>>,
    position: Duration,
    exact: bool,
    stop: &AtomicI32,
    started_at: &mut Instant,
    fallback_pts: &mut f64,
) -> Result<()> {
    native.seek(position, exact, stop)?;
    reset_frame_state(latest_frame);
    *started_at = Instant::now() - position;
    *fallback_pts = position.as_secs_f64();
    Ok(())
}
