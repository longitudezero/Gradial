//! Platform-independent building blocks for Gradial.

pub mod geometry;
pub mod paths;

pub use geometry::{Point, SliceCount};
pub use paths::{AppPaths, AppPathsError};
