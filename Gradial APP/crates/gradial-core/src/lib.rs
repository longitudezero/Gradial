//! Platform-independent building blocks for Gradial.

pub mod configuration;
pub mod geometry;
pub mod interaction;
pub mod paths;
pub mod profiles;

pub use configuration::{
    ApplicationProfile, CONFIG_SCHEMA_VERSION, Configuration, ConfigurationError, Menu, MenuSlice,
    Profile, ProfileRule,
};
pub use geometry::{Point, SliceCount};
pub use interaction::{
    ActivationBindingId, CancelReason, DEFAULT_OVERLAY_HALF_EXTENT_DIP, GeometryMode,
    GestureConfig, InputEvent, InputEventKind, InteractionController, InteractionEffect,
    InteractionOutcome, InteractionState, InvocationContext, InvocationGeometry,
    InvocationSnapshot, MonitorGeometry, PhysicalModifiers, ResolvedGeometry, ScreenPoint,
    ScreenRect, monitor_index_for_point, resolve_invocation_geometry,
};
pub use paths::{AppPaths, AppPathsError};
pub use profiles::{
    ProfileMatch, ProfileResolution, ProfileResolver, ProfileTarget, ResolutionDecision,
};
