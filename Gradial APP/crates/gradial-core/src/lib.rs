//! Platform-independent building blocks for Gradial.

pub mod geometry;
pub mod interaction;
pub mod paths;

pub use geometry::{Point, SliceCount};
pub use interaction::{
    CancelReason, GestureConfig, InputEvent, InputEventKind, InteractionController,
    InteractionEffect, InteractionOutcome, InteractionState, InvocationSnapshot, ScreenPoint,
};
pub use paths::{AppPaths, AppPathsError};
