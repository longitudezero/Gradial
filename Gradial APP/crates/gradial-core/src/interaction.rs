use std::f64::consts::{PI, TAU};

use uuid::Uuid;

use crate::SliceCount;

/// A signed physical-pixel coordinate in the Windows virtual screen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

impl ScreenPoint {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A fixed-size event copied out of a low-level input hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputEvent {
    pub qpc_timestamp: i64,
    pub point: ScreenPoint,
    pub kind: InputEventKind,
}

impl InputEvent {
    #[must_use]
    pub const fn new(qpc_timestamp: i64, point: ScreenPoint, kind: InputEventKind) -> Self {
        Self { qpc_timestamp, point, kind }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEventKind {
    ActivationDown,
    PointerMoved,
    ActivationUp,
    Cancel(CancelReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelReason {
    Escape,
    OtherMouseButton,
    NoSelection,
    InputQueueOverflow,
    HookUnresponsive,
    Shutdown,
}

/// Immutable information captured when M4 is pressed.
///
/// Milestone 0 captures the fields required by the gesture and focus checks.
/// Process identity and profile fields will be added with target validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationSnapshot {
    pub invocation_id: Uuid,
    pub qpc_timestamp: i64,
    pub foreground_window: usize,
    pub foreground_pid: u32,
    pub cursor_physical_px: ScreenPoint,
    pub monitor_dpi: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionState {
    Idle,
    Opening,
    Tracking,
    Cancelling,
    Closing,
    Faulted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionOutcome {
    Selected { slice_index: usize },
    Cancelled { reason: CancelReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionEffect {
    Open { invocation_id: Uuid, origin: ScreenPoint },
    SelectionChanged { invocation_id: Uuid, selected_slice: Option<usize> },
    Close { invocation_id: Uuid, outcome: InteractionOutcome },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureConfig {
    pub center_cancel_radius_dip: f64,
    pub selection_entry_radius_dip: f64,
    pub boundary_hysteresis_degrees: f64,
    pub slice_count: SliceCount,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            center_cancel_radius_dip: 24.0,
            selection_entry_radius_dip: 36.0,
            boundary_hysteresis_degrees: 5.0,
            slice_count: SliceCount::Eight,
        }
    }
}

#[derive(Debug)]
pub struct InteractionController {
    state: InteractionState,
    config: GestureConfig,
    snapshot: Option<InvocationSnapshot>,
    selected_slice: Option<usize>,
}

impl InteractionController {
    #[must_use]
    pub const fn new(config: GestureConfig) -> Self {
        Self { state: InteractionState::Idle, config, snapshot: None, selected_slice: None }
    }

    #[must_use]
    pub const fn state(&self) -> InteractionState {
        self.state
    }

    #[must_use]
    pub const fn selected_slice(&self) -> Option<usize> {
        self.selected_slice
    }

    #[must_use]
    pub const fn snapshot(&self) -> Option<InvocationSnapshot> {
        self.snapshot
    }

    pub fn activation_down(&mut self, snapshot: InvocationSnapshot) -> Option<InteractionEffect> {
        if self.state != InteractionState::Idle {
            return None;
        }

        self.state = InteractionState::Opening;
        self.selected_slice = None;
        self.snapshot = Some(snapshot);
        Some(InteractionEffect::Open {
            invocation_id: snapshot.invocation_id,
            origin: snapshot.cursor_physical_px,
        })
    }

    pub fn pointer_moved(&mut self, point: ScreenPoint) -> Option<InteractionEffect> {
        if !matches!(self.state, InteractionState::Opening | InteractionState::Tracking) {
            return None;
        }

        let Some(snapshot) = self.snapshot else {
            self.state = InteractionState::Faulted;
            return None;
        };
        let new_selection = self.selection_for_point(snapshot, point);
        if new_selection == self.selected_slice {
            return None;
        }

        self.selected_slice = new_selection;
        if new_selection.is_some() {
            self.state = InteractionState::Tracking;
        }

        Some(InteractionEffect::SelectionChanged {
            invocation_id: snapshot.invocation_id,
            selected_slice: new_selection,
        })
    }

    pub fn activation_up(&mut self) -> Option<InteractionEffect> {
        match self.state {
            InteractionState::Opening | InteractionState::Tracking => {
                let Some(snapshot) = self.snapshot else {
                    self.state = InteractionState::Faulted;
                    return None;
                };
                let outcome = self.selected_slice.map_or(
                    InteractionOutcome::Cancelled { reason: CancelReason::NoSelection },
                    |slice_index| InteractionOutcome::Selected { slice_index },
                );

                self.state = InteractionState::Closing;
                let effect =
                    InteractionEffect::Close { invocation_id: snapshot.invocation_id, outcome };
                self.reset_to_idle();
                Some(effect)
            }
            InteractionState::Cancelling => {
                self.reset_to_idle();
                None
            }
            _ => None,
        }
    }

    pub fn cancel(&mut self, reason: CancelReason) -> Option<InteractionEffect> {
        if !matches!(self.state, InteractionState::Opening | InteractionState::Tracking) {
            return None;
        }

        let Some(snapshot) = self.snapshot else {
            self.state = InteractionState::Faulted;
            return None;
        };
        self.state = InteractionState::Cancelling;
        self.selected_slice = None;
        Some(InteractionEffect::Close {
            invocation_id: snapshot.invocation_id,
            outcome: InteractionOutcome::Cancelled { reason },
        })
    }

    pub fn fault(&mut self, reason: CancelReason) -> Option<InteractionEffect> {
        let effect = self.snapshot.map(|snapshot| InteractionEffect::Close {
            invocation_id: snapshot.invocation_id,
            outcome: InteractionOutcome::Cancelled { reason },
        });
        self.state = InteractionState::Faulted;
        self.selected_slice = None;
        self.snapshot = None;
        effect
    }

    fn selection_for_point(
        &self,
        snapshot: InvocationSnapshot,
        point: ScreenPoint,
    ) -> Option<usize> {
        let dx = point.x - snapshot.cursor_physical_px.x;
        let dy = point.y - snapshot.cursor_physical_px.y;
        let dpi_scale = f64::from(snapshot.monitor_dpi) / 96.0;
        let distance = f64::from(dx).hypot(f64::from(dy));
        let center_radius = self.config.center_cancel_radius_dip * dpi_scale;
        let entry_radius = self.config.selection_entry_radius_dip * dpi_scale;

        if distance <= center_radius {
            return None;
        }
        if self.selected_slice.is_none() && distance < entry_radius {
            return None;
        }

        let candidate = slice_for_delta(dx, dy, self.config.slice_count);
        let Some(current) = self.selected_slice else {
            return Some(candidate);
        };
        if candidate == current {
            return Some(current);
        }

        let angle = clockwise_angle_from_north(dx, dy);
        let current_center = slice_center_angle(current, self.config.slice_count);
        let distance_from_current_center = shortest_angular_distance(angle, current_center).abs();
        let half_slice = half_slice_width(self.config.slice_count);
        let hysteresis = self.config.boundary_hysteresis_degrees.to_radians();

        if distance_from_current_center <= half_slice + hysteresis {
            Some(current)
        } else {
            Some(candidate)
        }
    }

    fn reset_to_idle(&mut self) {
        self.state = InteractionState::Idle;
        self.selected_slice = None;
        self.snapshot = None;
    }
}

/// Resolves a signed screen-space delta to a slice.
///
/// North is slice zero and indices increase clockwise. Exact boundaries select
/// the clockwise slice.
#[must_use]
pub fn slice_for_delta(dx: i32, dy: i32, slice_count: SliceCount) -> usize {
    let angle = clockwise_angle_from_north(dx, dy);
    slice_for_angle(angle, slice_count)
}

fn clockwise_angle_from_north(dx: i32, dy: i32) -> f64 {
    f64::from(dx).atan2(-f64::from(dy)).rem_euclid(TAU)
}

fn slice_center_angle(index: usize, slice_count: SliceCount) -> f64 {
    let index = u8::try_from(index).unwrap_or_default();
    f64::from(index) * slice_width(slice_count)
}

fn slice_for_angle(angle: f64, slice_count: SliceCount) -> usize {
    let width = slice_width(slice_count);
    let rotated = (angle + width / 2.0).rem_euclid(TAU);

    match slice_count {
        SliceCount::Four => {
            if rotated < width {
                0
            } else if rotated < width * 2.0 {
                1
            } else if rotated < width * 3.0 {
                2
            } else {
                3
            }
        }
        SliceCount::Eight => {
            if rotated < width {
                0
            } else if rotated < width * 2.0 {
                1
            } else if rotated < width * 3.0 {
                2
            } else if rotated < width * 4.0 {
                3
            } else if rotated < width * 5.0 {
                4
            } else if rotated < width * 6.0 {
                5
            } else if rotated < width * 7.0 {
                6
            } else {
                7
            }
        }
    }
}

const fn slice_width(slice_count: SliceCount) -> f64 {
    match slice_count {
        SliceCount::Four => std::f64::consts::FRAC_PI_2,
        SliceCount::Eight => std::f64::consts::FRAC_PI_4,
    }
}

const fn half_slice_width(slice_count: SliceCount) -> f64 {
    slice_width(slice_count) / 2.0
}

fn shortest_angular_distance(left: f64, right: f64) -> f64 {
    (left - right + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use super::{
        CancelReason, GestureConfig, InteractionController, InteractionEffect, InteractionOutcome,
        InteractionState, InvocationSnapshot, ScreenPoint, slice_for_angle, slice_for_delta,
    };
    use crate::SliceCount;
    use uuid::Uuid;

    fn snapshot(origin: ScreenPoint) -> InvocationSnapshot {
        InvocationSnapshot {
            invocation_id: Uuid::nil(),
            qpc_timestamp: 100,
            foreground_window: 42,
            foreground_pid: 7,
            cursor_physical_px: origin,
            monitor_dpi: 96,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn point_at_angle(origin: ScreenPoint, angle_degrees: f64, radius: f64) -> ScreenPoint {
        let angle = angle_degrees.to_radians();
        ScreenPoint::new(
            origin.x + (angle.sin() * radius).round() as i32,
            origin.y - (angle.cos() * radius).round() as i32,
        )
    }

    #[test]
    fn quick_press_without_selection_cancels() {
        let origin = ScreenPoint::new(20, 30);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller.activation_down(snapshot(origin)).expect("open");

        assert_eq!(
            controller.activation_up(),
            Some(InteractionEffect::Close {
                invocation_id: Uuid::nil(),
                outcome: InteractionOutcome::Cancelled { reason: CancelReason::NoSelection },
            })
        );
        assert_eq!(controller.state(), InteractionState::Idle);
    }

    #[test]
    fn north_flick_selects_and_releases() {
        let origin = ScreenPoint::new(-500, -300);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller.activation_down(snapshot(origin)).expect("open");

        assert_eq!(
            controller.pointer_moved(ScreenPoint::new(-500, -340)),
            Some(InteractionEffect::SelectionChanged {
                invocation_id: Uuid::nil(),
                selected_slice: Some(0),
            })
        );
        assert_eq!(
            controller.activation_up(),
            Some(InteractionEffect::Close {
                invocation_id: Uuid::nil(),
                outcome: InteractionOutcome::Selected { slice_index: 0 },
            })
        );
    }

    #[test]
    fn returning_to_center_clears_selection() {
        let origin = ScreenPoint::new(100, 100);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller.activation_down(snapshot(origin)).expect("open");
        controller.pointer_moved(ScreenPoint::new(160, 100)).expect("east selected");

        assert_eq!(
            controller.pointer_moved(ScreenPoint::new(110, 100)),
            Some(InteractionEffect::SelectionChanged {
                invocation_id: Uuid::nil(),
                selected_slice: None,
            })
        );
    }

    #[test]
    fn hysteresis_retains_current_slice_until_five_degrees_past_boundary() {
        let origin = ScreenPoint::new(0, 0);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller.activation_down(snapshot(origin)).expect("open");
        controller.pointer_moved(point_at_angle(origin, 0.0, 10_000.0)).expect("north selected");

        assert_eq!(controller.pointer_moved(point_at_angle(origin, 27.4, 10_000.0)), None);
        assert_eq!(controller.selected_slice(), Some(0));

        assert_eq!(
            controller.pointer_moved(point_at_angle(origin, 27.6, 10_000.0)),
            Some(InteractionEffect::SelectionChanged {
                invocation_id: Uuid::nil(),
                selected_slice: Some(1),
            })
        );
    }

    #[test]
    fn escape_closes_once_and_waits_for_activation_release() {
        let origin = ScreenPoint::new(0, 0);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller.activation_down(snapshot(origin)).expect("open");

        assert!(matches!(
            controller.cancel(CancelReason::Escape),
            Some(InteractionEffect::Close {
                outcome: InteractionOutcome::Cancelled { reason: CancelReason::Escape },
                ..
            })
        ));
        assert_eq!(controller.state(), InteractionState::Cancelling);
        assert_eq!(controller.cancel(CancelReason::Escape), None);
        assert_eq!(controller.activation_up(), None);
        assert_eq!(controller.state(), InteractionState::Idle);
    }

    #[test]
    fn eight_slice_centers_and_boundaries_are_deterministic() {
        let origin = ScreenPoint::new(-10_000, -20_000);
        let cases = [
            (0.0, 0),
            (45.0, 1),
            (90.0, 2),
            (135.0, 3),
            (180.0, 4),
            (225.0, 5),
            (270.0, 6),
            (315.0, 7),
            (22.4, 0),
            (22.6, 1),
            (337.4, 7),
            (337.6, 0),
        ];

        for (angle, expected) in cases {
            let point = point_at_angle(origin, angle, 10_000.0);
            assert_eq!(
                slice_for_delta(point.x - origin.x, point.y - origin.y, SliceCount::Eight),
                expected,
                "angle {angle}"
            );
        }
    }

    #[test]
    fn exact_eight_slice_boundaries_choose_the_clockwise_slice() {
        let half_slice = std::f64::consts::FRAC_PI_8;
        assert_eq!(slice_for_angle(half_slice, SliceCount::Eight), 1);
        assert_eq!(slice_for_angle(-half_slice, SliceCount::Eight), 0);
    }

    #[test]
    fn four_slice_centers_and_boundaries_are_deterministic() {
        let origin = ScreenPoint::new(-4_000, 3_000);
        let cases = [
            (0.0, 0),
            (90.0, 1),
            (180.0, 2),
            (270.0, 3),
            (44.9, 0),
            (45.0, 1),
            (45.1, 1),
            (314.9, 3),
            (315.0, 0),
            (315.1, 0),
        ];

        for (angle, expected) in cases {
            let point = point_at_angle(origin, angle, 10_000.0);
            assert_eq!(
                slice_for_delta(point.x - origin.x, point.y - origin.y, SliceCount::Four),
                expected,
                "angle {angle}"
            );
        }
    }
}
