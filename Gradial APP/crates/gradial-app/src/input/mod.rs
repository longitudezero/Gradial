mod hooks;
mod raw_input;

use gradial_core::{GeometryMode, MonitorGeometry};
pub(crate) use hooks::{HookThread, HookThreadMetrics};

/// Marker reserved for synthetic input emitted by Gradial.
///
/// The input hooks pass events carrying this value straight through without
/// feeding them back into the interaction engine.
pub(crate) const GRADIAL_INJECTED_MARKER: usize = 0x4752_444C_494E_5054;

#[derive(Clone, Debug)]
pub(crate) struct InputConfiguration {
    pub(crate) geometry_mode: GeometryMode,
    pub(crate) monitors: Vec<MonitorGeometry>,
}
