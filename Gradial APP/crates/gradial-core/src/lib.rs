//! Platform-independent building blocks for Gradial.

pub mod geometry;
pub mod interaction;
pub mod paths;

pub use geometry::{Point, SliceCount};
pub use interaction::{
    CancelReason, DEFAULT_OVERLAY_HALF_EXTENT_DIP, GeometryMode, GestureConfig, InputEvent,
    InputEventKind, InteractionController, InteractionEffect, InteractionOutcome, InteractionState,
    InvocationGeometry, InvocationSnapshot, MonitorGeometry, ResolvedGeometry, ScreenPoint,
    ScreenRect, monitor_index_for_point, resolve_invocation_geometry,
};
pub use paths::{AppPaths, AppPathsError};
