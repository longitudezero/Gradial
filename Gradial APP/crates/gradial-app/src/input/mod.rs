mod binding_test;
mod hooks;
mod raw_input;

pub(crate) use binding_test::{
    ActivationBindingTestResult, begin as begin_activation_binding_test,
    cancel as cancel_activation_binding_test, is_gesture_active as activation_test_gesture_active,
    take_result as take_activation_binding_test_result,
};
use gradial_core::{ActivationBinding, GeometryMode, MonitorGeometry};
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
    pub(crate) activation_binding: ActivationBinding,
    pub(crate) overlay_half_extent_dip: u32,
}
