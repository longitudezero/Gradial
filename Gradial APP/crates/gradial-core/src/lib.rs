//! Platform-independent building blocks for Gradial.

pub mod actions;
pub mod configuration;
pub mod geometry;
pub mod interaction;
pub mod paths;
pub mod profiles;

pub use actions::{
    ActionDefinition, ActionErrorCode, ActionKind, ActionPlanCompiler, ActionPlanError,
    ActionResult, DEFAULT_ACTION_STEP_TIMEOUT_MS, ExecutionOperation, ExecutionPlan, ExecutionStep,
    MAX_ACTION_SEQUENCE_DEPTH, MAX_ACTION_SEQUENCE_DURATION_MS, MAX_ACTION_SEQUENCE_STEPS,
    MAX_TYPED_TEXT_BYTES, ShortcutKey, ShortcutModifier, WindowCommand,
};
pub use configuration::{
    ACTION_CONFIG_SCHEMA_VERSION, ACTIVATION_CONFIG_SCHEMA_VERSION, AccentColor, Appearance,
    ApplicationProfile, CONFIG_SCHEMA_VERSION, Configuration, ConfigurationError,
    LEGACY_CONFIG_SCHEMA_VERSION, LabelTheme, Menu, MenuSlice, Profile, ProfileRule, SizePreset,
};
pub use geometry::{Point, SliceCount};
pub use interaction::{
    ActivationBinding, ActivationBindingId, CancelReason, DEFAULT_OVERLAY_HALF_EXTENT_DIP,
    GeometryMode, GestureConfig, InputEvent, InputEventKind, InteractionController,
    InteractionEffect, InteractionOutcome, InteractionState, InvocationContext, InvocationGeometry,
    InvocationSnapshot, MonitorGeometry, PhysicalModifiers, ResolvedGeometry, ScreenPoint,
    ScreenRect, monitor_index_for_point, resolve_invocation_geometry, slice_for_delta,
};
pub use paths::{AppPaths, AppPathsError};
pub use profiles::{
    ProfileMatch, ProfileResolution, ProfileResolver, ProfileTarget, ResolutionDecision,
    ResolvedMenu,
};
