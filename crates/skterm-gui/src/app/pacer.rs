//! Frame pacing: keeping PTY-driven repaints down to what the display can show.

use super::events::{DEFAULT_FRAME_INTERVAL, FRAME_INTERVAL_REFRESH};
use std::time::{Duration, Instant};

/// What the caller should do about a deferred frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacerPoll {
    /// Nothing outstanding.
    Idle,
    /// A deferred frame is now due -- repaint.
    RedrawNow,
    /// A frame is owed at this instant; arm the event loop's timer for it.
    WakeAt(Instant),
}

/// Rate limiter for repaints caused by PTY output.
///
/// A shell dumping output makes the pty readable every few milliseconds and each
/// wakeup used to turn straight into a frame, far faster than the monitor could
/// show them -- so most of that rendering was discarded by the compositor, and
/// the main thread spent its time blocked in `get_current_texture` instead of
/// answering input.
///
/// Deliberately **leading edge**: once a period's worth of time has passed we
/// draw immediately and defer only the surplus. That distinction is what keeps
/// typing sharp. Keystroke latency in a terminal runs through the pty echo path
/// -- key down, write to the pty, the shell echoes it back, repaint -- so it is
/// this type, not the keyboard handler, that decides how fast a typed character
/// appears. After any idle moment the elapsed time is well past the interval and
/// the echo is drawn on the spot; throttling only engages mid-flood, where frames
/// were already arriving faster than the display could present them. A
/// trailing-edge throttle would have added up to a full refresh period to every
/// keystroke.
///
/// Time is taken as a parameter rather than read from the clock so the whole
/// state machine is testable; see the tests at the bottom of this file.
#[derive(Debug)]
pub struct FramePacer {
    /// Start of the most recent frame.
    last_frame: Instant,
    /// A repaint was held back and still owes a frame.
    pending: bool,
    /// Minimum gap between frames: one refresh period of the current monitor.
    interval: Duration,
    /// When `interval` was last re-read from the monitor.
    interval_checked: Instant,
}

impl FramePacer {
    pub fn new(interval: Duration, now: Instant) -> Self {
        Self {
            last_frame: now,
            pending: false,
            interval,
            interval_checked: now,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// PTY output changed something visible.
    ///
    /// Returns `true` when the caller should repaint right away; otherwise the
    /// frame is recorded as owed and [`FramePacer::poll`] will hand it back.
    pub fn on_output(&mut self, now: Instant) -> bool {
        if now.duration_since(self.last_frame) >= self.interval {
            self.pending = false;
            true
        } else {
            self.pending = true;
            false
        }
    }

    /// Idle-loop check: release a deferred frame, or say when it comes due.
    ///
    /// Without this the tail of a burst would never be drawn -- output stops, no
    /// further wakeup arrives, and the frame the pacer held back would sit
    /// pending forever with the screen showing stale content.
    pub fn poll(&mut self, now: Instant) -> PacerPoll {
        if !self.pending {
            return PacerPoll::Idle;
        }
        let due = self.last_frame + self.interval;
        if now >= due {
            self.pending = false;
            PacerPoll::RedrawNow
        } else {
            PacerPoll::WakeAt(due)
        }
    }

    /// A frame is starting now.
    ///
    /// Stamped for *every* frame whatever caused it, since the pacer is about how
    /// often this window paints rather than about pty output specifically.
    ///
    /// Start-to-start on purpose. Stamping after present would make the cadence
    /// `interval + frame_time`, so a 5 ms frame on a 60 Hz panel would settle at
    /// ~46 fps -- throttling below the refresh rate instead of to it. Measuring
    /// from the start caps us at the display's rate while frames are cheap and
    /// stops throttling altogether once a frame costs more than a refresh period,
    /// which is the right behaviour: at that point the renderer, not the pacer,
    /// is the limit.
    ///
    /// Clearing `pending` here is not just tidiness. A frame drawn for an
    /// unrelated reason (a keystroke, a resize) also puts the pending output on
    /// screen, so leaving the flag set would schedule a wakeup to draw a frame
    /// identical to this one.
    pub fn on_frame_start(&mut self, now: Instant) {
        self.last_frame = now;
        self.pending = false;
    }

    /// Whether the monitor's refresh rate is due to be re-read.
    ///
    /// Moving a window from a 60 Hz panel to a 120 Hz one would otherwise keep it
    /// paced at 60, which is visible. Polling beats listening for `Moved`: that
    /// event fires continuously while a window is dragged, and it would still
    /// miss the user changing the rate in display settings.
    ///
    /// A window that has not painted in the last [`FRAME_INTERVAL_REFRESH`] is not
    /// being paced, so there is nothing to keep accurate and the platform round
    /// trip is skipped; it picks the new rate up within one interval of becoming
    /// active again.
    pub fn interval_refresh_due(&self, now: Instant) -> bool {
        now.duration_since(self.last_frame) <= FRAME_INTERVAL_REFRESH
            && now.duration_since(self.interval_checked) >= FRAME_INTERVAL_REFRESH
    }

    pub fn set_interval(&mut self, interval: Duration, now: Instant) {
        self.interval = interval;
        self.interval_checked = now;
    }
}

/// Convert a monitor's reported rate into a frame period.
///
/// Falls back to [`DEFAULT_FRAME_INTERVAL`] when the platform reports nothing
/// (some Wayland compositors, headless setups) or reports zero, which would
/// otherwise divide by zero.
pub fn frame_interval_from_millihertz(millihertz: Option<u32>) -> Duration {
    match millihertz {
        // period[ns] = 1e12 / rate[mHz]; 60000 mHz -> 16_666_666 ns.
        Some(mhz) if mhz > 0 => Duration::from_nanos(1_000_000_000_000 / u64::from(mhz)),
        _ => DEFAULT_FRAME_INTERVAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `frame_interval_from_millihertz` yields for a real 60 Hz panel.
    /// Not `from_micros(16_667)`: rounding the period up loses a frame a second,
    /// which is exactly the kind of drift these tests exist to catch.
    const HZ60: Duration = Duration::from_nanos(16_666_666);

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn frame_interval_matches_common_refresh_rates() {
        // The literal in the conversion is 1e12, and getting it wrong by a factor
        // of a thousand still yields a plausible-looking Duration, so pin the
        // rates that actually ship on hardware.
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        assert!((ms(frame_interval_from_millihertz(Some(60_000))) - 16.667).abs() < 0.01);
        // The tests below hardcode HZ60; keep it honest.
        assert_eq!(frame_interval_from_millihertz(Some(60_000)), HZ60);
        assert!((ms(frame_interval_from_millihertz(Some(120_000))) - 8.333).abs() < 0.01);
        assert!((ms(frame_interval_from_millihertz(Some(144_000))) - 6.944).abs() < 0.01);
        // ProMotion idling at its floor.
        assert!((ms(frame_interval_from_millihertz(Some(24_000))) - 41.667).abs() < 0.01);
    }

    #[test]
    fn frame_interval_falls_back_when_the_rate_is_unusable() {
        // A zero rate would panic on divide-by-zero rather than merely mispace.
        assert_eq!(frame_interval_from_millihertz(None), DEFAULT_FRAME_INTERVAL);
        assert_eq!(
            frame_interval_from_millihertz(Some(0)),
            DEFAULT_FRAME_INTERVAL
        );
        assert!((DEFAULT_FRAME_INTERVAL.as_secs_f64() * 1000.0 - 16.667).abs() < 0.01);
    }

    #[test]
    fn output_after_an_idle_gap_draws_immediately() {
        // The typing path. A keystroke's echo must never wait for the pacer.
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);
        assert!(p.on_output(at(t0, 500)));
        assert_eq!(p.poll(at(t0, 500)), PacerPoll::Idle);
    }

    #[test]
    fn typing_is_never_throttled_at_realistic_key_rates() {
        // macOS's fastest key repeat is one character per ~15 ms and a fast
        // human tops out far below that, so every echo should be drawn on
        // arrival. Walk 60 keystrokes and assert not one of them is deferred.
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);
        for i in 1..=60u64 {
            let now = at(t0, i * 25);
            assert!(p.on_output(now), "keystroke {i} was deferred");
            p.on_frame_start(now);
        }
    }

    #[test]
    fn a_burst_draws_once_then_defers() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);

        // First chunk lands after an idle gap -> leading edge fires.
        assert!(p.on_output(at(t0, 100)));
        p.on_frame_start(at(t0, 100));

        // The rest of the burst arrives within the same refresh period.
        for ms in [101, 103, 105, 110, 116] {
            assert!(!p.on_output(at(t0, ms)), "{ms}ms should have been deferred");
        }

        // Still inside the period: the loop is told when to come back.
        assert_eq!(
            p.poll(at(t0, 110)),
            PacerPoll::WakeAt(at(t0, 100) + HZ60),
            "a deferred frame must hand back a deadline, or the loop would sleep"
        );
    }

    #[test]
    fn a_burst_that_stops_still_gets_its_final_frame() {
        // The regression this guards: output stops mid-period, so no further pty
        // wakeup ever arrives. If `poll` did not release the held frame, the tail
        // of the burst would stay invisible until the *next* unrelated event --
        // which during a quiet moment could be seconds later, or never.
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);
        p.on_frame_start(at(t0, 100));
        assert!(!p.on_output(at(t0, 105)));

        // Output has now stopped for good. Only the timer runs.
        assert_eq!(p.poll(at(t0, 106)), PacerPoll::WakeAt(at(t0, 100) + HZ60));
        assert_eq!(p.poll(at(t0, 117)), PacerPoll::RedrawNow);
        // And it is released exactly once.
        assert_eq!(p.poll(at(t0, 118)), PacerPoll::Idle);
    }

    #[test]
    fn a_frame_drawn_for_another_reason_absorbs_the_pending_one() {
        // A keystroke or resize repaint shows the pending output too, so the
        // pacer must not schedule a second, identical frame.
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);
        p.on_frame_start(at(t0, 100));
        assert!(!p.on_output(at(t0, 105)));
        p.on_frame_start(at(t0, 108));
        assert_eq!(p.poll(at(t0, 200)), PacerPoll::Idle);
    }

    /// Run the pacer through a simulated flood and count the frames it lets out.
    ///
    /// This mirrors the real loop rather than stepping on a fixed grid: after
    /// every event `about_to_wait` polls the pacer, and the loop then sleeps
    /// until whichever comes first, the next pty wakeup or the deadline the pacer
    /// asked to have armed. Stepping on a fixed 1 ms grid instead would report 58
    /// frames for a 60 Hz panel purely because 16.667 ms does not divide into
    /// whole milliseconds -- an artefact of the harness, not of the pacer.
    fn simulate_flood(interval: Duration, output_every: Duration, run_for: Duration) -> usize {
        let t0 = Instant::now();
        let end = t0 + run_for;
        let mut pacer = FramePacer::new(interval, t0);
        let mut frames = 0usize;
        let mut now = t0;
        let mut next_output = t0;

        while now < end {
            if now >= next_output {
                if pacer.on_output(now) {
                    frames += 1;
                    pacer.on_frame_start(now);
                }
                next_output = now + output_every;
            }
            let armed = match pacer.poll(now) {
                PacerPoll::RedrawNow => {
                    frames += 1;
                    pacer.on_frame_start(now);
                    None
                }
                PacerPoll::WakeAt(due) => Some(due),
                PacerPoll::Idle => None,
            };
            now = match armed {
                Some(due) => due.min(next_output),
                None => next_output,
            };
        }
        frames
    }

    #[test]
    fn sustained_output_settles_at_exactly_one_frame_per_interval() {
        // A 1 ms-per-chunk flood -- output far faster than any display -- must
        // come out at exactly the panel's rate. Drifting below it would show as
        // stuttery scrolling; above it is the waste this whole change removes.
        let frames = simulate_flood(HZ60, Duration::from_millis(1), Duration::from_secs(1));
        assert_eq!(frames, 60, "expected 60 frames in a simulated second");
    }

    #[test]
    fn the_monitor_rate_is_what_sets_the_cap() {
        // Guards against 60 being baked in anywhere downstream of the conversion.
        let hz120 = frame_interval_from_millihertz(Some(120_000));
        let frames = simulate_flood(hz120, Duration::from_millis(1), Duration::from_secs(1));
        assert_eq!(frames, 120);

        let hz24 = frame_interval_from_millihertz(Some(24_000));
        let frames = simulate_flood(hz24, Duration::from_millis(1), Duration::from_secs(1));
        assert_eq!(frames, 24);
    }

    #[test]
    fn output_slower_than_the_display_is_never_held_back() {
        // 30 chunks a second on a 60 Hz panel: every one should be drawn on
        // arrival, so the pacer must be invisible here.
        let frames = simulate_flood(HZ60, Duration::from_millis(33), Duration::from_millis(990));
        assert_eq!(frames, 30);
    }

    #[test]
    fn slow_frames_stop_the_pacer_from_throttling_further() {
        // When a frame costs more than a refresh period the renderer is already
        // the bottleneck; the pacer must get out of the way rather than stack its
        // own delay on top.
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);
        p.on_frame_start(at(t0, 0));
        // 40 ms frame. The next output arrives once it is done.
        assert!(p.on_output(at(t0, 40)));
    }

    #[test]
    fn the_refresh_rate_is_rechecked_while_active_and_skipped_while_idle() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(HZ60, t0);

        // Freshly checked -> not due.
        assert!(!p.interval_refresh_due(at(t0, 500)));

        // Active (painting) and a second has passed -> due.
        p.on_frame_start(at(t0, 1_400));
        assert!(p.interval_refresh_due(at(t0, 1_500)));

        // Idle for longer than the refresh window -> skip the platform call.
        assert!(!p.interval_refresh_due(at(t0, 5_000)));

        p.set_interval(frame_interval_from_millihertz(Some(120_000)), at(t0, 1_500));
        assert_eq!(p.interval(), Duration::from_nanos(8_333_333));
        assert!(!p.interval_refresh_due(at(t0, 1_600)));
    }
}
