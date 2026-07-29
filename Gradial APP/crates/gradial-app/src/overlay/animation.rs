use std::time::Duration;

pub(crate) const OPEN_DURATION: Duration = Duration::from_millis(60);
pub(crate) const SELECTION_DURATION: Duration = Duration::from_millis(35);
pub(crate) const CLOSE_DURATION: Duration = Duration::from_millis(50);
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(8);

const OPEN_START_SCALE: f32 = 0.96;
const OPEN_START_OPACITY: f32 = 0.85;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AnimationMode {
    #[default]
    Standard,
    ReducedMotion,
}

impl AnimationMode {
    pub(crate) const fn is_reduced(self) -> bool {
        matches!(self, Self::ReducedMotion)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PresenceFrame {
    pub(crate) opacity: f32,
    pub(crate) scale: f32,
    pub(crate) complete: bool,
    pub(crate) repaint_after: Option<Duration>,
}

pub(crate) fn opening_frame(elapsed: Duration, mode: AnimationMode) -> PresenceFrame {
    let progress = duration_progress(elapsed, OPEN_DURATION);
    let complete = progress >= 1.0;
    if mode.is_reduced() {
        return PresenceFrame {
            opacity: 1.0,
            scale: 1.0,
            complete,
            repaint_after: remaining_repaint(elapsed, OPEN_DURATION),
        };
    }

    let eased = ease_out_cubic(progress);
    PresenceFrame {
        opacity: lerp(OPEN_START_OPACITY, 1.0, eased),
        scale: lerp(OPEN_START_SCALE, 1.0, eased),
        complete,
        repaint_after: animation_repaint(complete),
    }
}

pub(crate) fn closing_frame(elapsed: Duration, mode: AnimationMode) -> PresenceFrame {
    let progress = duration_progress(elapsed, CLOSE_DURATION);
    let complete = progress >= 1.0;
    PresenceFrame {
        opacity: if mode.is_reduced() { 0.0 } else { 1.0 - progress },
        scale: 1.0,
        complete,
        repaint_after: if mode.is_reduced() {
            remaining_repaint(elapsed, CLOSE_DURATION)
        } else {
            animation_repaint(complete)
        },
    }
}

pub(crate) fn selection_progress(elapsed: Duration) -> f32 {
    duration_progress(elapsed, SELECTION_DURATION)
}

fn duration_progress(elapsed: Duration, duration: Duration) -> f32 {
    if elapsed >= duration { 1.0 } else { elapsed.as_secs_f32() / duration.as_secs_f32() }
}

fn remaining_repaint(elapsed: Duration, duration: Duration) -> Option<Duration> {
    duration.checked_sub(elapsed)
}

const fn animation_repaint(complete: bool) -> Option<Duration> {
    if complete { None } else { Some(FRAME_INTERVAL) }
}

const fn ease_out_cubic(value: f32) -> f32 {
    let inverse = 1.0 - value;
    1.0 - inverse * inverse * inverse
}

const fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        AnimationMode, CLOSE_DURATION, OPEN_DURATION, SELECTION_DURATION, closing_frame,
        opening_frame, selection_progress,
    };

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn opening_begins_usable_and_finishes_at_sixty_milliseconds() {
        let first = opening_frame(Duration::ZERO, AnimationMode::Standard);
        assert_close(first.opacity, 0.85);
        assert_close(first.scale, 0.96);
        assert!(!first.complete);

        let last = opening_frame(OPEN_DURATION, AnimationMode::Standard);
        assert_close(last.opacity, 1.0);
        assert_close(last.scale, 1.0);
        assert!(last.complete);
        assert_eq!(last.repaint_after, None);
    }

    #[test]
    fn closing_reaches_zero_at_fifty_milliseconds() {
        let middle = closing_frame(Duration::from_millis(25), AnimationMode::Standard);
        assert_close(middle.opacity, 0.5);
        assert!(!middle.complete);

        let last = closing_frame(CLOSE_DURATION, AnimationMode::Standard);
        assert_close(last.opacity, 0.0);
        assert!(last.complete);
    }

    #[test]
    fn selection_transition_is_bounded_to_thirty_five_milliseconds() {
        assert_close(selection_progress(Duration::ZERO), 0.0);
        assert_close(selection_progress(SELECTION_DURATION), 1.0);
        assert_close(selection_progress(Duration::from_millis(70)), 1.0);
    }

    #[test]
    fn reduced_motion_snaps_visuals_without_shortening_lifecycle() {
        let opening = opening_frame(Duration::ZERO, AnimationMode::ReducedMotion);
        assert_eq!((opening.opacity, opening.scale), (1.0, 1.0));
        assert!(!opening.complete);
        assert_eq!(opening.repaint_after, Some(OPEN_DURATION));

        let closing = closing_frame(Duration::ZERO, AnimationMode::ReducedMotion);
        assert_close(closing.opacity, 0.0);
        assert!(!closing.complete);
        assert_eq!(closing.repaint_after, Some(CLOSE_DURATION));
    }
}
