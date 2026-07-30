use std::{
    f64::consts::{PI, TAU},
    path::PathBuf,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SliceCount;

pub const DEFAULT_OVERLAY_HALF_EXTENT_DIP: u32 = 144;

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

/// Startup behavior used to compare the Milestone 0 edge prototypes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GeometryMode {
    #[default]
    Auto,
    FixedOrigin,
    EdgeAssist,
}

impl GeometryMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::FixedOrigin => "fixed-origin",
            Self::EdgeAssist => "edge-assist",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedGeometry {
    FixedOrigin,
    EdgeAssist,
}

impl ResolvedGeometry {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FixedOrigin => "fixed-origin",
            Self::EdgeAssist => "edge-assist",
        }
    }
}

/// A signed, right-and-bottom-exclusive physical-pixel rectangle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    #[must_use]
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    #[must_use]
    pub const fn contains(self, point: ScreenPoint) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    #[must_use]
    pub const fn centered_content_fits(self, point: ScreenPoint, half_extent_px: i32) -> bool {
        point.x.saturating_sub(half_extent_px) >= self.left
            && point.x.saturating_add(half_extent_px) <= self.right
            && point.y.saturating_sub(half_extent_px) >= self.top
            && point.y.saturating_add(half_extent_px) <= self.bottom
    }

    #[must_use]
    pub const fn inset_center(self, point: ScreenPoint, half_extent_px: i32) -> ScreenPoint {
        let minimum_x = self.left.saturating_add(half_extent_px);
        let maximum_x = self.right.saturating_sub(half_extent_px);
        let minimum_y = self.top.saturating_add(half_extent_px);
        let maximum_y = self.bottom.saturating_sub(half_extent_px);

        ScreenPoint::new(
            clamp_or_midpoint(point.x, minimum_x, maximum_x),
            clamp_or_midpoint(point.y, minimum_y, maximum_y),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MonitorGeometry {
    pub bounds: ScreenRect,
    pub work_area: ScreenRect,
    pub dpi: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationGeometry {
    pub behavior: ResolvedGeometry,
    pub overlay_origin_physical_px: ScreenPoint,
    pub monitor_dpi: u32,
}

impl InvocationGeometry {
    #[must_use]
    pub const fn fixed(origin: ScreenPoint, monitor_dpi: u32) -> Self {
        Self {
            behavior: ResolvedGeometry::FixedOrigin,
            overlay_origin_physical_px: origin,
            monitor_dpi,
        }
    }
}

/// Resolves the visual origin and gesture behavior without doing Win32 work.
#[must_use]
pub fn resolve_invocation_geometry(
    mode: GeometryMode,
    cursor: ScreenPoint,
    monitors: &[MonitorGeometry],
    overlay_half_extent_dip: u32,
) -> InvocationGeometry {
    let Some(monitor_index) = monitor_index_for_point(cursor, monitors) else {
        return InvocationGeometry::fixed(cursor, 96);
    };
    let monitor = monitors[monitor_index];
    let half_extent_px = scale_dip_ceil(overlay_half_extent_dip, monitor.dpi);
    let fixed_fits = monitor.work_area.centered_content_fits(cursor, half_extent_px);
    let behavior = match mode {
        GeometryMode::Auto if fixed_fits => ResolvedGeometry::FixedOrigin,
        GeometryMode::FixedOrigin => ResolvedGeometry::FixedOrigin,
        GeometryMode::Auto | GeometryMode::EdgeAssist => ResolvedGeometry::EdgeAssist,
    };
    let overlay_origin_physical_px = match behavior {
        ResolvedGeometry::FixedOrigin => cursor,
        ResolvedGeometry::EdgeAssist => monitor.work_area.inset_center(cursor, half_extent_px),
    };

    InvocationGeometry { behavior, overlay_origin_physical_px, monitor_dpi: monitor.dpi.max(96) }
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
    ActivationDown {
        geometry: InvocationGeometry,
        physical_modifiers: PhysicalModifiers,
        activation_binding: ActivationBindingId,
    },
    PointerMoved,
    RawPointerDelta {
        dx: i32,
        dy: i32,
    },
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
    InputReregistration,
    DisplayTopologyChange,
    Shutdown,
}

/// Immutable information captured when the activation button is pressed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvocationSnapshot {
    pub invocation_id: Uuid,
    pub qpc_timestamp: i64,
    pub cursor_physical_px: ScreenPoint,
    pub monitor_dpi: u32,
    pub geometry: InvocationGeometry,
    pub context: Arc<InvocationContext>,
}

/// Target, display, binding, and configuration identity frozen at activation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InvocationContext {
    pub foreground_window: usize,
    pub foreground_pid: u32,
    pub process_start_time: u64,
    pub executable_path: Option<PathBuf>,
    pub window_class: String,
    pub window_title: String,
    pub monitor_device_name: String,
    pub monitor_rect_physical_px: ScreenRect,
    pub work_area_physical_px: ScreenRect,
    pub physical_modifiers: PhysicalModifiers,
    pub activation_binding: ActivationBindingId,
    pub config_revision: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ActivationBindingId {
    #[serde(rename = "m4")]
    #[default]
    MouseButton4,
    #[serde(rename = "m5")]
    MouseButton5,
}

impl ActivationBindingId {
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::MouseButton4 => "M4",
            Self::MouseButton5 => "M5",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationBinding {
    pub button: ActivationBindingId,
    pub modifiers: PhysicalModifiers,
}

impl ActivationBinding {
    #[must_use]
    pub fn matches(
        self,
        button: ActivationBindingId,
        physical_modifiers: PhysicalModifiers,
    ) -> bool {
        self.button == button
            && self.modifiers.control == physical_modifiers.control
            && self.modifiers.alt == physical_modifiers.alt
            && self.modifiers.shift == physical_modifiers.shift
    }
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
    active_slice_count: SliceCount,
    snapshot: Option<InvocationSnapshot>,
    selected_slice: Option<usize>,
    virtual_pointer: ScreenPoint,
}

impl InteractionController {
    #[must_use]
    pub const fn new(config: GestureConfig) -> Self {
        Self {
            state: InteractionState::Idle,
            active_slice_count: config.slice_count,
            config,
            snapshot: None,
            selected_slice: None,
            virtual_pointer: ScreenPoint::new(0, 0),
        }
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
    pub fn snapshot(&self) -> Option<&InvocationSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn activation_down(&mut self, snapshot: InvocationSnapshot) -> Option<InteractionEffect> {
        self.activation_down_with_slice_count(snapshot, self.config.slice_count)
    }

    pub fn activation_down_with_slice_count(
        &mut self,
        snapshot: InvocationSnapshot,
        slice_count: SliceCount,
    ) -> Option<InteractionEffect> {
        if self.state != InteractionState::Idle {
            return None;
        }

        self.state = InteractionState::Opening;
        self.active_slice_count = slice_count;
        self.selected_slice = None;
        self.virtual_pointer = snapshot.cursor_physical_px;
        let invocation_id = snapshot.invocation_id;
        let origin = snapshot.geometry.overlay_origin_physical_px;
        self.snapshot = Some(snapshot);
        Some(InteractionEffect::Open { invocation_id, origin })
    }

    pub fn raw_pointer_delta(&mut self, dx: i32, dy: i32) -> Option<InteractionEffect> {
        if self.snapshot.as_ref().map(|snapshot| snapshot.geometry.behavior)
            != Some(ResolvedGeometry::EdgeAssist)
        {
            return None;
        }

        self.virtual_pointer = ScreenPoint::new(
            self.virtual_pointer.x.saturating_add(dx),
            self.virtual_pointer.y.saturating_add(dy),
        );
        self.pointer_moved(self.virtual_pointer)
    }

    pub fn pointer_moved(&mut self, point: ScreenPoint) -> Option<InteractionEffect> {
        if !matches!(self.state, InteractionState::Opening | InteractionState::Tracking) {
            return None;
        }

        let Some(snapshot) = self.snapshot.as_ref() else {
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
                let Some(snapshot) = self.snapshot.as_ref() else {
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

        let Some(snapshot) = self.snapshot.as_ref() else {
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
        let effect = self.snapshot.as_ref().map(|snapshot| InteractionEffect::Close {
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
        snapshot: &InvocationSnapshot,
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

        let candidate = slice_for_delta(dx, dy, self.active_slice_count);
        let Some(current) = self.selected_slice else {
            return Some(candidate);
        };
        if candidate == current {
            return Some(current);
        }

        let angle = clockwise_angle_from_north(dx, dy);
        let current_center = slice_center_angle(current, self.active_slice_count);
        let distance_from_current_center = shortest_angular_distance(angle, current_center).abs();
        let half_slice = half_slice_width(self.active_slice_count);
        let hysteresis = self.config.boundary_hysteresis_degrees.to_radians();

        if distance_from_current_center <= half_slice + hysteresis {
            Some(current)
        } else {
            Some(candidate)
        }
    }

    fn reset_to_idle(&mut self) {
        self.state = InteractionState::Idle;
        self.active_slice_count = self.config.slice_count;
        self.selected_slice = None;
        self.snapshot = None;
        self.virtual_pointer = ScreenPoint::new(0, 0);
    }
}

const fn clamp_or_midpoint(value: i32, minimum: i32, maximum: i32) -> i32 {
    if minimum <= maximum {
        if value < minimum {
            minimum
        } else if value > maximum {
            maximum
        } else {
            value
        }
    } else {
        minimum.saturating_add(maximum).div_euclid(2)
    }
}

#[must_use]
pub fn monitor_index_for_point(point: ScreenPoint, monitors: &[MonitorGeometry]) -> Option<usize> {
    monitors.iter().position(|monitor| monitor.bounds.contains(point)).or_else(|| {
        monitors
            .iter()
            .enumerate()
            .min_by_key(|(_, monitor)| distance_to_rect(point, monitor.bounds))
            .map(|(index, _)| index)
    })
}

fn distance_to_rect(point: ScreenPoint, rectangle: ScreenRect) -> i64 {
    let dx = if point.x < rectangle.left {
        rectangle.left.saturating_sub(point.x)
    } else if point.x >= rectangle.right {
        point.x.saturating_sub(rectangle.right.saturating_sub(1))
    } else {
        0
    };
    let dy = if point.y < rectangle.top {
        rectangle.top.saturating_sub(point.y)
    } else if point.y >= rectangle.bottom {
        point.y.saturating_sub(rectangle.bottom.saturating_sub(1))
    } else {
        0
    };

    i64::from(dx).saturating_mul(i64::from(dx)) + i64::from(dy).saturating_mul(i64::from(dy))
}

fn scale_dip_ceil(dip: u32, dpi: u32) -> i32 {
    let scaled = dip.saturating_mul(dpi.max(96)).saturating_add(95) / 96;
    i32::try_from(scaled).unwrap_or(i32::MAX)
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
        ActivationBinding, ActivationBindingId, CancelReason, GeometryMode, GestureConfig,
        InteractionController, InteractionEffect, InteractionOutcome, InteractionState,
        InvocationGeometry, InvocationSnapshot, MonitorGeometry, PhysicalModifiers,
        ResolvedGeometry, ScreenPoint, ScreenRect, monitor_index_for_point,
        resolve_invocation_geometry, slice_for_angle, slice_for_delta,
    };
    use crate::SliceCount;
    use uuid::Uuid;

    fn snapshot(origin: ScreenPoint) -> InvocationSnapshot {
        InvocationSnapshot {
            invocation_id: Uuid::nil(),
            qpc_timestamp: 100,
            cursor_physical_px: origin,
            monitor_dpi: 96,
            geometry: InvocationGeometry::fixed(origin, 96),
            context: std::sync::Arc::new(super::InvocationContext {
                foreground_window: 42,
                foreground_pid: 7,
                ..super::InvocationContext::default()
            }),
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
    fn activation_binding_requires_the_exact_generic_modifier_set() {
        let binding = ActivationBinding {
            button: ActivationBindingId::MouseButton5,
            modifiers: PhysicalModifiers { control: true, alt: false, shift: true },
        };

        assert!(binding.matches(
            ActivationBindingId::MouseButton5,
            PhysicalModifiers { control: true, alt: false, shift: true },
        ));
        assert!(!binding.matches(
            ActivationBindingId::MouseButton4,
            PhysicalModifiers { control: true, alt: false, shift: true },
        ));
        assert!(!binding.matches(
            ActivationBindingId::MouseButton5,
            PhysicalModifiers { control: true, alt: true, shift: true },
        ));
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
    fn resolved_slice_count_is_frozen_per_invocation() {
        let origin = ScreenPoint::new(200, 300);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller
            .activation_down_with_slice_count(snapshot(origin), SliceCount::Four)
            .expect("four-slice open");

        assert_eq!(
            controller.pointer_moved(ScreenPoint::new(260, 300)),
            Some(InteractionEffect::SelectionChanged {
                invocation_id: Uuid::nil(),
                selected_slice: Some(1),
            })
        );
        assert_eq!(
            controller.activation_up(),
            Some(InteractionEffect::Close {
                invocation_id: Uuid::nil(),
                outcome: InteractionOutcome::Selected { slice_index: 1 },
            })
        );

        controller
            .activation_down_with_slice_count(snapshot(origin), SliceCount::Eight)
            .expect("eight-slice open");
        assert_eq!(
            controller.pointer_moved(ScreenPoint::new(260, 300)),
            Some(InteractionEffect::SelectionChanged {
                invocation_id: Uuid::nil(),
                selected_slice: Some(2),
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
    fn automatic_geometry_keeps_center_invocations_fixed() {
        let monitor = MonitorGeometry {
            bounds: ScreenRect::new(0, 0, 1_920, 1_080),
            work_area: ScreenRect::new(0, 0, 1_920, 1_040),
            dpi: 96,
        };

        assert_eq!(
            resolve_invocation_geometry(
                GeometryMode::Auto,
                ScreenPoint::new(960, 520),
                &[monitor],
                144,
            ),
            InvocationGeometry {
                behavior: ResolvedGeometry::FixedOrigin,
                overlay_origin_physical_px: ScreenPoint::new(960, 520),
                monitor_dpi: 96,
            }
        );
    }

    #[test]
    fn monitor_routing_handles_negative_coordinates_and_nearest_fallback() {
        let monitors = [
            MonitorGeometry {
                bounds: ScreenRect::new(-1_920, 0, 0, 1_080),
                work_area: ScreenRect::new(-1_920, 0, 0, 1_040),
                dpi: 96,
            },
            MonitorGeometry {
                bounds: ScreenRect::new(0, 0, 2_560, 1_440),
                work_area: ScreenRect::new(0, 0, 2_560, 1_400),
                dpi: 144,
            },
        ];

        assert_eq!(monitor_index_for_point(ScreenPoint::new(-500, 200), &monitors), Some(0));
        assert_eq!(monitor_index_for_point(ScreenPoint::new(500, 200), &monitors), Some(1));
        assert_eq!(monitor_index_for_point(ScreenPoint::new(3_000, 200), &monitors), Some(1));
        assert_eq!(monitor_index_for_point(ScreenPoint::default(), &[]), None);
    }

    #[test]
    fn automatic_geometry_insets_all_corners_with_negative_coordinates() {
        let monitor = MonitorGeometry {
            bounds: ScreenRect::new(-2_560, -1_440, 0, 0),
            work_area: ScreenRect::new(-2_560, -1_400, 0, 0),
            dpi: 192,
        };
        let cases = [
            (ScreenPoint::new(-2_559, -1_399), ScreenPoint::new(-2_272, -1_112)),
            (ScreenPoint::new(-1, -1_399), ScreenPoint::new(-288, -1_112)),
            (ScreenPoint::new(-2_559, -1), ScreenPoint::new(-2_272, -288)),
            (ScreenPoint::new(-1, -1), ScreenPoint::new(-288, -288)),
        ];

        for (cursor, expected_origin) in cases {
            let geometry = resolve_invocation_geometry(GeometryMode::Auto, cursor, &[monitor], 144);
            assert_eq!(geometry.behavior, ResolvedGeometry::EdgeAssist);
            assert_eq!(geometry.overlay_origin_physical_px, expected_origin);
            assert_eq!(geometry.monitor_dpi, 192);
        }
    }

    #[test]
    fn comparison_overrides_force_each_geometry() {
        let monitor = MonitorGeometry {
            bounds: ScreenRect::new(0, 0, 1_000, 800),
            work_area: ScreenRect::new(0, 0, 1_000, 760),
            dpi: 96,
        };
        let corner = ScreenPoint::new(2, 2);
        let center = ScreenPoint::new(500, 380);

        assert_eq!(
            resolve_invocation_geometry(GeometryMode::FixedOrigin, corner, &[monitor], 144,)
                .behavior,
            ResolvedGeometry::FixedOrigin
        );
        assert_eq!(
            resolve_invocation_geometry(GeometryMode::EdgeAssist, center, &[monitor], 144,)
                .behavior,
            ResolvedGeometry::EdgeAssist
        );
    }

    #[test]
    fn raw_deltas_drive_all_edge_assist_directions_without_moving_the_origin() {
        let origin = ScreenPoint::new(0, 0);
        let mut edge_snapshot = snapshot(origin);
        edge_snapshot.geometry = InvocationGeometry {
            behavior: ResolvedGeometry::EdgeAssist,
            overlay_origin_physical_px: ScreenPoint::new(144, 144),
            monitor_dpi: 96,
        };

        let deltas = [
            ((0, -60), 0),
            ((60, -60), 1),
            ((60, 0), 2),
            ((60, 60), 3),
            ((0, 60), 4),
            ((-60, 60), 5),
            ((-60, 0), 6),
            ((-60, -60), 7),
        ];

        for ((dx, dy), expected_slice) in deltas {
            let mut controller = InteractionController::new(GestureConfig::default());
            assert_eq!(
                controller.activation_down(edge_snapshot.clone()),
                Some(InteractionEffect::Open {
                    invocation_id: Uuid::nil(),
                    origin: ScreenPoint::new(144, 144),
                })
            );
            assert_eq!(
                controller.raw_pointer_delta(dx, dy),
                Some(InteractionEffect::SelectionChanged {
                    invocation_id: Uuid::nil(),
                    selected_slice: Some(expected_slice),
                })
            );
            assert_eq!(controller.snapshot(), Some(&edge_snapshot));
        }
    }

    #[test]
    fn fixed_origin_ignores_raw_deltas() {
        let origin = ScreenPoint::new(500, 500);
        let mut controller = InteractionController::new(GestureConfig::default());
        controller.activation_down(snapshot(origin)).expect("open");

        assert_eq!(controller.raw_pointer_delta(100, 0), None);
        assert_eq!(controller.selected_slice(), None);
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
