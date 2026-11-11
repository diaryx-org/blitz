//! Wheel and trackpad scrolling, moved once a frame (PATCH(diaryx)).
//!
//! Upstream dispatches every winit `MouseWheel` the moment it arrives, so a
//! page scrolls as often as the input device reports rather than as often as
//! the display refreshes: a trackpad reporting at 60 Hz scrolls at 60 Hz on a
//! 120 Hz screen, and unevenly, as reports and frames drift against each
//! other. A mouse wheel's notch jumps a whole line in one frame, and a
//! trackpad stops dead when the fingers lift, where the rest of the desktop
//! glides on.
//!
//! Where the platform does none of this for the app ([`SMOOTH`]: Linux and
//! the BSDs; macOS sends momentum of its own, and a phone scrolls by touch),
//! the shell hands each wheel event to a [`WheelSmoother`] and dispatches what
//! [`WheelSmoother::tick`] gives it once a frame, as one wheel event:
//!
//! - A trackpad's movement is replayed a report behind, its path sampled at
//!   each frame's time, so the page moves at the display's rate and evenly,
//!   whatever rate the trackpad reports at.
//! - When the fingers lift (Wayland's `axis_stop`, winit's
//!   [`TouchPhase::Ended`]), the scroll glides on at the speed they had and
//!   slows as blitz-dom's touch fling does: UIKit's deceleration, 0.998 of the
//!   speed kept a millisecond. Two fingers set down again, a press, the
//!   pointer moving, or any new scrolling stops it.
//! - A mouse wheel's notch is eased out over about 75 ms, as a browser does.
//!
//! Everything given in is given out: the smoother changes when a page
//! scrolls, never how far, except for the glide.

use std::collections::VecDeque;

use blitz_traits::events::BlitzWheelDelta;
use winit::event::TouchPhase;

/// Whether wheel input goes through a [`WheelSmoother`] on this platform.
pub(crate) const SMOOTH: bool = cfg!(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd",
));

/// The speed a glide keeps each millisecond: UIKit's
/// `UIScrollView.DecelerationRate.normal`, as blitz-dom's touch fling.
const DECELERATION: f64 = 0.998;
/// A glide starts only from fingers moving at least this fast, in CSS pixels
/// a millisecond (50 a second).
const MIN_GLIDE_SPEED: f64 = 0.05;
/// A glide ends when it has slowed to this, as the touch fling does.
const STOP_SPEED: f64 = 0.01;
/// The fingers' speed when they lift is their movement over this last stretch.
const VELOCITY_WINDOW_MS: f64 = 100.0;
/// A trackpad gesture with no report for this long has ended without saying
/// so, as a backend that reports no phases, or a mouse with a smooth wheel,
/// leaves it.
const GESTURE_TIMEOUT_MS: f64 = 100.0;
/// How far behind the replay runs before the device's rate is known: between
/// a 60 Hz and a 120 Hz report.
const DEFAULT_LAG_MS: f64 = 12.0;
/// The replay runs a report and a fifth behind...
const LAG_MARGIN: f64 = 1.2;
/// ...but never less than this...
const MIN_LAG_MS: f64 = 4.0;
/// ...nor more than this, a slow device's reports are sampled late instead.
const MAX_LAG_MS: f64 = 34.0;
/// A notch's ease-out: what remains of it decays with this time constant, so
/// 95% of it has moved within three of them.
const NOTCH_TAU_MS: f64 = 25.0;
/// The longest step of a notch's ease one frame takes, so that a frame after
/// a stall does not leap. (A glide is worked out from the time since the
/// fingers lifted, so a late frame moves as far as the frames it missed.)
const MAX_FRAME_MS: f64 = 50.0;

/// One frame's scrolling: a wheel event for each kind of delta that moved.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct WheelFrame {
    pub lines: Option<(f64, f64)>,
    pub pixels: Option<(f64, f64)>,
}

impl WheelFrame {
    pub fn deltas(self) -> impl Iterator<Item = BlitzWheelDelta> {
        let lines = self.lines.map(|(x, y)| BlitzWheelDelta::Lines(x, y));
        let pixels = self.pixels.map(|(x, y)| BlitzWheelDelta::Pixels(x, y));
        lines.into_iter().chain(pixels)
    }
}

/// A point on a trackpad gesture's path: the total it had moved by `t`.
#[derive(Clone, Copy, Debug)]
struct Sample {
    t: f64,
    x: f64,
    y: f64,
}

/// A trackpad gesture being replayed, and the glide after it.
#[derive(Debug)]
struct Gesture {
    /// The path, oldest first; the first is where the replay has reached or
    /// before it.
    samples: VecDeque<Sample>,
    /// How much of the path has been given out, and the time on it the
    /// replay has reached.
    given: (f64, f64),
    replayed: f64,
    /// When the fingers lifted, and the speed they lifted at, in CSS pixels a
    /// millisecond. The glide runs from the end of the path at that speed.
    lifted: Option<(f64, (f64, f64))>,
    /// How much of the glide has been given out.
    glided: (f64, f64),
}

impl Gesture {
    fn end(&self) -> Sample {
        *self.samples.back().expect("a gesture has a sample")
    }

    /// Where the path is at `t`, the straight line between the reports
    /// either side; its end once `t` is past it.
    fn at(&self, t: f64) -> (f64, f64) {
        let first = self.samples[0];
        if t <= first.t {
            return (first.x, first.y);
        }
        for pair in self.samples.iter().zip(self.samples.iter().skip(1)) {
            let (a, b) = pair;
            if t <= b.t {
                let f = if b.t > a.t {
                    (t - a.t) / (b.t - a.t)
                } else {
                    1.0
                };
                return (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
            }
        }
        let end = self.end();
        (end.x, end.y)
    }

    /// How far the glide has gone `dt` milliseconds after the path's end.
    fn glide(v: (f64, f64), dt: f64) -> (f64, f64) {
        let travel = (DECELERATION.powf(dt) - 1.0) / DECELERATION.ln();
        (v.0 * travel, v.1 * travel)
    }

    /// The glide's speed `dt` milliseconds after the path's end.
    fn glide_speed(v: (f64, f64), dt: f64) -> f64 {
        v.0.abs().max(v.1.abs()) * DECELERATION.powf(dt)
    }

    /// The speed over the last stretch of the path, on its main axis alone,
    /// as the touch fling takes it: a vertical scroll's sideways wobble does
    /// not glide.
    fn lift_speed(&self) -> (f64, f64) {
        let end = self.end();
        let start = self
            .samples
            .iter()
            .find(|s| end.t - s.t <= VELOCITY_WINDOW_MS)
            .copied()
            .unwrap_or(end);
        let dt = end.t - start.t;
        if dt <= 0.0 {
            return (0.0, 0.0);
        }
        let (vx, vy) = ((end.x - start.x) / dt, (end.y - start.y) / dt);
        if vx.abs() > vy.abs() {
            (vx, 0.0)
        } else {
            (0.0, vy)
        }
    }
}

/// A mouse wheel's notches still to be eased out, in lines.
#[derive(Debug)]
struct Notches {
    left: (f64, f64),
    /// When the ease last moved.
    t: f64,
}

/// Wheel input in, one frame's scrolling out. Times are milliseconds on any
/// clock that only moves forward; the shell's is `Instant`.
#[derive(Debug, Default)]
pub(crate) struct WheelSmoother {
    gesture: Option<Gesture>,
    notches: Option<Notches>,
    /// The trackpad's interval between reports, as last seen.
    lag: Option<f64>,
    /// Given out at the next frame as it is: what was left of a gesture or
    /// an ease that another kind of input cut short.
    flush: WheelFrame,
}

impl WheelSmoother {
    /// Whether the coming frames may have anything to give out, at `now`.
    /// While they may, the shell keeps drawing: through a glide, and while
    /// the fingers are on the trackpad, so that the frame after a report is
    /// already asked for when it comes. Fingers resting on the trackpad ask
    /// for none once they have sent nothing for [`GESTURE_TIMEOUT_MS`].
    pub fn is_active(&self, now: f64) -> bool {
        let moving = self.gesture.as_ref().is_some_and(|g| {
            g.lifted.is_some() || g.replayed < g.end().t || now - g.end().t < GESTURE_TIMEOUT_MS
        });
        moving || self.notches.is_some() || self.flush != WheelFrame::default()
    }

    /// One wheel event, at `now`.
    pub fn push(&mut self, now: f64, delta: BlitzWheelDelta, phase: TouchPhase) {
        match delta {
            BlitzWheelDelta::Pixels(x, y) => self.push_pixels(now, x, y, phase),
            BlitzWheelDelta::Lines(x, y) => self.push_lines(now, x, y),
        }
    }

    fn push_pixels(&mut self, now: f64, x: f64, y: f64, phase: TouchPhase) {
        self.finish_notches();
        // A glide is cut short by new movement, and a new gesture starts
        // where it was.
        let fresh = match &self.gesture {
            None => true,
            Some(g) => {
                g.lifted.is_some()
                    || phase == TouchPhase::Started
                    || now - g.end().t >= GESTURE_TIMEOUT_MS
            }
        };
        if fresh {
            self.finish_gesture();
            let lag = self.lag.unwrap_or(DEFAULT_LAG_MS);
            self.gesture = Some(Gesture {
                samples: VecDeque::from([Sample {
                    t: now - lag,
                    x: 0.0,
                    y: 0.0,
                }]),
                given: (0.0, 0.0),
                replayed: now - lag,
                lifted: None,
                glided: (0.0, 0.0),
            });
        }
        let g = self.gesture.as_mut().expect("a gesture was just made");
        let end = g.end();
        if !fresh {
            // Reports that arrived in one batch say nothing of the rate.
            let gap = now - end.t;
            if (1.0..GESTURE_TIMEOUT_MS).contains(&gap) {
                // A fifth more than the interval, so that a report a little
                // late does not find the replay already caught up and still.
                let lag = match self.lag {
                    Some(lag) => lag * 0.8 + gap * LAG_MARGIN * 0.2,
                    None => gap * LAG_MARGIN,
                };
                self.lag = Some(lag.clamp(MIN_LAG_MS, MAX_LAG_MS));
            }
        }
        g.samples.push_back(Sample {
            t: now.max(end.t),
            x: end.x + x,
            y: end.y + y,
        });
        if phase == TouchPhase::Ended {
            let v = g.lift_speed();
            let v = if Gesture::glide_speed(v, 0.0) >= MIN_GLIDE_SPEED {
                v
            } else {
                (0.0, 0.0)
            };
            g.lifted = Some((g.end().t, v));
        }
    }

    fn push_lines(&mut self, now: f64, x: f64, y: f64) {
        self.finish_gesture();
        match &mut self.notches {
            Some(n) => {
                n.left.0 += x;
                n.left.1 += y;
            }
            None => {
                self.notches = Some(Notches {
                    left: (x, y),
                    t: now,
                })
            }
        }
    }

    /// Stops a glide where it is, as two fingers set down on the trackpad
    /// or a press do. The fingers' own movement is still given out.
    pub fn stop_glide(&mut self) {
        if let Some(g) = &mut self.gesture
            && let Some((_, v)) = &mut g.lifted
        {
            *v = (0.0, 0.0);
            g.glided = (0.0, 0.0);
        }
    }

    /// Gives out everything still queued at the next frame, with no glide:
    /// for input that must not wait behind it.
    pub fn finish(&mut self) {
        self.finish_gesture();
        self.finish_notches();
    }

    fn finish_gesture(&mut self) {
        if let Some(g) = self.gesture.take() {
            let end = g.end();
            add(
                &mut self.flush.pixels,
                (end.x - g.given.0, end.y - g.given.1),
            );
        }
    }

    fn finish_notches(&mut self) {
        if let Some(n) = self.notches.take() {
            add(&mut self.flush.lines, n.left);
        }
    }

    /// The scrolling for a frame drawn at `now`.
    pub fn tick(&mut self, now: f64) -> WheelFrame {
        let mut frame = std::mem::take(&mut self.flush);

        if let Some(g) = &mut self.gesture {
            let lag = self.lag.unwrap_or(DEFAULT_LAG_MS);
            let replay = now - lag;
            let (x, y) = g.at(replay);
            add(&mut frame.pixels, (x - g.given.0, y - g.given.1));
            g.given = (x, y);
            g.replayed = g.replayed.max(replay);
            // Keep the one report at or before the replay and those after,
            // and the last stretch the fingers' speed is read over.
            while g.samples.len() > 1
                && g.samples[1].t <= replay
                && g.end().t - g.samples[1].t > VELOCITY_WINDOW_MS
            {
                g.samples.pop_front();
            }

            let end = g.end();
            let done = match g.lifted {
                Some((lifted_at, v)) => {
                    let since = (replay - lifted_at).max(0.0);
                    if since > 0.0 && Gesture::glide_speed(v, 0.0) > 0.0 {
                        let (gx, gy) = Gesture::glide(v, since);
                        add(&mut frame.pixels, (gx - g.glided.0, gy - g.glided.1));
                        g.glided = (gx, gy);
                    }
                    replay >= end.t && Gesture::glide_speed(v, since) < STOP_SPEED
                }
                // Ended without saying so.
                None => replay >= end.t && now - end.t >= GESTURE_TIMEOUT_MS,
            };
            if done {
                self.gesture = None;
            }
        }

        if let Some(n) = &mut self.notches {
            let dt = (now - n.t).clamp(0.0, MAX_FRAME_MS);
            n.t = now;
            let kept = (-dt / NOTCH_TAU_MS).exp();
            let step = (n.left.0 * (1.0 - kept), n.left.1 * (1.0 - kept));
            let rest = (n.left.0 - step.0, n.left.1 - step.1);
            if rest.0.abs() < 0.005 && rest.1.abs() < 0.005 {
                add(&mut frame.lines, n.left);
                self.notches = None;
            } else {
                add(&mut frame.lines, step);
                n.left = rest;
            }
        }

        // Nothing that does not move.
        for delta in [&mut frame.lines, &mut frame.pixels] {
            if delta.is_some_and(|(x, y)| x == 0.0 && y == 0.0) {
                *delta = None;
            }
        }
        frame
    }
}

fn add(to: &mut Option<(f64, f64)>, d: (f64, f64)) {
    let (x, y) = to.unwrap_or_default();
    *to = Some((x + d.0, y + d.1));
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f64 = 1000.0 / 120.0;

    fn px(y: f64) -> BlitzWheelDelta {
        BlitzWheelDelta::Pixels(0.0, y)
    }

    /// Runs frames from `from` to `to`, and the vertical pixels each gave out.
    fn frames(s: &mut WheelSmoother, from: f64, to: f64) -> Vec<f64> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to {
            out.push(s.tick(t).pixels.map_or(0.0, |p| p.1));
            t += FRAME;
        }
        out
    }

    /// A trackpad reporting every `every` ms, `step` pixels a report, from
    /// `0` to `until`, its frames at 120 Hz between: the pixels each frame
    /// gave out.
    fn steady(s: &mut WheelSmoother, every: f64, step: f64, until: f64, lift: bool) -> Vec<f64> {
        let mut out = Vec::new();
        let mut next_report = 0.0;
        let mut next_frame = 0.5;
        let mut first = true;
        while next_report <= until || next_frame <= until {
            if next_report <= next_frame && next_report <= until {
                let last = next_report + every > until;
                let phase = if first {
                    TouchPhase::Started
                } else if last && lift {
                    TouchPhase::Ended
                } else {
                    TouchPhase::Moved
                };
                s.push(next_report, px(step), phase);
                first = false;
                next_report += every;
            } else {
                out.push(s.tick(next_frame).pixels.map_or(0.0, |p| p.1));
                next_frame += FRAME;
            }
        }
        out
    }

    #[test]
    fn a_60hz_trackpad_moves_the_page_every_120hz_frame_evenly() {
        let mut s = WheelSmoother::default();
        // 2 px a millisecond, reported every 16.7 ms.
        let out = steady(&mut s, 1000.0 / 60.0, 2000.0 / 60.0, 500.0, false);
        // Once the replay has caught the device's rate, every frame moves,
        // and by the same amount: a frame's worth of 2 px/ms.
        let settled = &out[10..out.len() - 4];
        for d in settled {
            assert!((d - 2.0 * FRAME).abs() < 0.5, "uneven frames: {settled:?}");
        }
    }

    #[test]
    fn a_gesture_gives_out_all_it_moved_and_no_more_without_a_lift() {
        let mut s = WheelSmoother::default();
        let mut out = steady(&mut s, 11.0, 7.0, 300.0, false);
        out.extend(frames(&mut s, 301.0, 600.0));
        let reports = (300.0f64 / 11.0).floor() + 1.0;
        let total: f64 = out.iter().sum();
        assert!(
            (total - reports * 7.0).abs() < 1e-6,
            "{total} != {}",
            reports * 7.0
        );
        assert!(!s.is_active(1e9));
    }

    #[test]
    fn lifted_fingers_glide_on_and_slow_to_a_stop() {
        let mut s = WheelSmoother::default();
        // 1.5 px/ms, then lifted.
        let mut out = steady(&mut s, 10.0, 15.0, 200.0, true);
        let moved: f64 = out.iter().sum();
        let glide = frames(&mut s, 200.5, 6000.0);
        out.extend(&glide);
        // The glide goes about v / -ln(0.998) further: ~750 px at 1.5 px/ms.
        let glided: f64 = out.iter().sum::<f64>() - 21.0 * 15.0;
        assert!(
            glided > 650.0 && glided < 760.0,
            "glided {glided}, moved {moved}"
        );
        // It slows: each frame moves no more than the one before, after the
        // fingers' own movement is replayed.
        let tail: Vec<f64> = glide.iter().copied().filter(|d| *d > 0.0).collect();
        for w in tail.windows(2).skip(3) {
            assert!(w[1] <= w[0] + 1e-9, "a glide frame sped up: {w:?}");
        }
        assert!(!s.is_active(1e9), "the glide never stopped");
    }

    #[test]
    fn a_slow_lift_does_not_glide() {
        let mut s = WheelSmoother::default();
        // 0.02 px/ms: a careful drag to place a line.
        let mut out = steady(&mut s, 10.0, 0.2, 200.0, true);
        out.extend(frames(&mut s, 200.5, 1000.0));
        let total: f64 = out.iter().sum();
        assert!((total - 21.0 * 0.2).abs() < 1e-6, "{total}");
        assert!(!s.is_active(1e9));
    }

    #[test]
    fn new_scrolling_cuts_a_glide_short() {
        let mut s = WheelSmoother::default();
        steady(&mut s, 10.0, 15.0, 200.0, true);
        frames(&mut s, 200.5, 400.0);
        assert!(s.is_active(400.0));
        // The fingers come back, the other way.
        s.push(400.0, px(-5.0), TouchPhase::Started);
        let out = frames(&mut s, 400.5, 600.0);
        assert!(out.iter().all(|d| *d <= 0.0), "the glide went on: {out:?}");
        assert!((out.iter().sum::<f64>() + 5.0).abs() < 1e-6);
    }

    #[test]
    fn a_stopped_glide_moves_no_further() {
        let mut s = WheelSmoother::default();
        steady(&mut s, 10.0, 15.0, 200.0, true);
        frames(&mut s, 200.5, 300.0);
        s.stop_glide();
        let out = frames(&mut s, 300.5, 2000.0);
        assert!(out.iter().all(|d| *d == 0.0), "{out:?}");
        assert!(!s.is_active(1e9));
    }

    #[test]
    fn resting_fingers_keep_frames_coming_only_for_a_while() {
        let mut s = WheelSmoother::default();
        s.push(0.0, px(10.0), TouchPhase::Started);
        s.push(16.0, px(10.0), TouchPhase::Moved);
        frames(&mut s, 0.5, 60.0);
        // Caught up with the reports, but the next may come at any frame.
        assert!(s.is_active(60.0));
        assert!(!s.is_active(16.0 + GESTURE_TIMEOUT_MS));
    }

    #[test]
    fn a_scroll_ended_early_gives_out_what_was_moved_and_no_glide() {
        let mut s = WheelSmoother::default();
        let before = steady(&mut s, 10.0, 15.0, 200.0, true);
        // The replay is still a report behind, and the glide not begun.
        s.stop_glide();
        s.finish();
        let now = s.tick(201.0).pixels.map_or(0.0, |p| p.1);
        let after = frames(&mut s, 202.0, 2000.0);
        assert!(after.iter().all(|d| *d == 0.0), "{after:?}");
        assert!(!s.is_active(2000.0));
        // All 21 reports' movement, given out once, and nothing more.
        let total = before.iter().sum::<f64>() + now;
        assert!((total - 21.0 * 15.0).abs() < 1e-6, "{total}");
    }

    #[test]
    fn a_notch_eases_out_to_exactly_its_lines() {
        let mut s = WheelSmoother::default();
        s.push(0.0, BlitzWheelDelta::Lines(0.0, -3.0), TouchPhase::Moved);
        let mut moved = Vec::new();
        let mut t = 0.0;
        while s.is_active(t) && t < 1000.0 {
            t += FRAME;
            moved.push(s.tick(t).lines.map_or(0.0, |l| l.1));
        }
        assert!((moved.iter().sum::<f64>() + 3.0).abs() < 1e-9);
        // Over several frames, the largest first.
        assert!(moved.len() >= 5, "{moved:?}");
        assert!(moved[0] < moved[1]);
        // Done once less than 0.005 lines is left: 3 lines in about
        // ln(600) time constants.
        assert!(t < 7.0 * NOTCH_TAU_MS, "took {t} ms");
    }

    #[test]
    fn a_wheel_notch_finishes_a_trackpad_gesture_first() {
        let mut s = WheelSmoother::default();
        s.push(0.0, px(10.0), TouchPhase::Started);
        s.push(10.0, px(10.0), TouchPhase::Moved);
        s.push(12.0, BlitzWheelDelta::Lines(0.0, 1.0), TouchPhase::Moved);
        let f = s.tick(13.0);
        assert_eq!(f.pixels, Some((0.0, 20.0)));
        assert!(f.lines.is_some_and(|l| l.1 > 0.0));
    }
}
