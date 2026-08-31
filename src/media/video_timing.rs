use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) const DISPLAY_RATE_WINDOW: Duration = Duration::from_secs(2);
pub(super) const VIDEO_CLOCK_LEAD: Duration = Duration::from_millis(5);
pub(super) const VIDEO_CLOCK_DROP_LAG: Duration = Duration::from_millis(75);
pub(super) const CLOCK_DROP_STARVATION_LIMIT: Duration = Duration::from_millis(200);
pub(super) const MAX_CONSECUTIVE_CLOCK_DROPS: u32 = 8;

#[derive(Default)]
pub(super) struct DisplayRate {
    pub(super) delivered_at: VecDeque<Instant>,
}

impl DisplayRate {
    pub(super) fn record(&mut self, now: Instant) {
        self.delivered_at.push_back(now);
        let cutoff = now.checked_sub(DISPLAY_RATE_WINDOW).unwrap_or(now);
        while self
            .delivered_at
            .front()
            .is_some_and(|sample| *sample < cutoff)
        {
            self.delivered_at.pop_front();
        }
    }

    pub(super) fn measured_at(&self, now: Instant) -> Option<f64> {
        let cutoff = now.checked_sub(DISPLAY_RATE_WINDOW).unwrap_or(now);
        let mut samples = self
            .delivered_at
            .iter()
            .copied()
            .filter(|sample| *sample >= cutoff);
        let first = samples.next()?;
        let mut last = first;
        let mut intervals = 0_u32;
        for sample in samples {
            last = sample;
            intervals = intervals.saturating_add(1);
        }
        let elapsed = last.saturating_duration_since(first).as_secs_f64();
        (intervals > 0 && elapsed > 0.0).then_some(f64::from(intervals) / elapsed)
    }
}

pub(super) fn master_clock_position(
    master_clock: &Mutex<Option<Arc<AtomicI64>>>,
) -> Option<Duration> {
    let clock = master_clock.lock().ok()?.clone()?;
    let micros = clock.load(Ordering::Acquire);
    (micros >= 0).then(|| Duration::from_micros(micros as u64))
}

pub(super) fn stale_frame_drop_before(
    master_clock: &Mutex<Option<Arc<AtomicI64>>>,
) -> Option<Duration> {
    master_clock_position(master_clock)
        .and_then(|position| position.checked_sub(VIDEO_CLOCK_DROP_LAG))
}

pub(super) fn should_publish_late_frame(
    consecutive_clock_drops: u32,
    last_published_at: Instant,
    now: Instant,
) -> bool {
    consecutive_clock_drops >= MAX_CONSECUTIVE_CLOCK_DROPS
        || now.saturating_duration_since(last_published_at) >= CLOCK_DROP_STARVATION_LIMIT
}

#[cfg(test)]
#[path = "tests/video_timing.rs"]
mod tests;
