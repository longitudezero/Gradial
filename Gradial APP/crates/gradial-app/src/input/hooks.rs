use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::bounded;
use gradial_core::{
    CancelReason, DEFAULT_OVERLAY_HALF_EXTENT_DIP, InputEvent, InputEventKind, ResolvedGeometry,
    ScreenPoint, resolve_invocation_geometry,
};
use rtrb::Producer;
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{Performance::QueryPerformanceCounter, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::VK_ESCAPE,
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
            KillTimer, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SetTimer, SetWindowsHookExW,
            TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
            WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
            WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
            WM_XBUTTONDOWN, WM_XBUTTONUP,
        },
    },
};

use super::{GRADIAL_INJECTED_MARKER, InputConfiguration, raw_input::RawInputWindow};

const HEARTBEAT_INTERVAL_MS: u32 = 250;
const HEARTBEAT_TIMER_ID: usize = 0x4752_4842;
const XBUTTON1: u16 = 1;
const XBUTTON2: u16 = 2;
const SUPPRESSED_LEFT: u8 = 1 << 0;
const SUPPRESSED_RIGHT: u8 = 1 << 1;
const SUPPRESSED_MIDDLE: u8 = 1 << 2;
const SUPPRESSED_XBUTTON2: u8 = 1 << 3;

thread_local! {
    static CALLBACK_STATE: RefCell<Option<HookCallbackState>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub(crate) struct HookThreadMetrics {
    pub(crate) heartbeat_qpc: Arc<AtomicI64>,
    pub(crate) queue_overflow: Arc<AtomicBool>,
    pub(crate) maximum_callback_qpc: Arc<AtomicI64>,
    #[cfg(debug_assertions)]
    pub(crate) debug: DebugHookMetrics,
}

impl HookThreadMetrics {
    #[must_use]
    fn new() -> Self {
        Self {
            heartbeat_qpc: Arc::new(AtomicI64::new(0)),
            queue_overflow: Arc::new(AtomicBool::new(false)),
            maximum_callback_qpc: Arc::new(AtomicI64::new(0)),
            #[cfg(debug_assertions)]
            debug: DebugHookMetrics::new(),
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug)]
pub(crate) struct DebugHookMetrics {
    pub(crate) mouse_callback_count: Arc<std::sync::atomic::AtomicU64>,
    pub(crate) last_mouse_message: Arc<std::sync::atomic::AtomicU32>,
    pub(crate) last_mouse_data: Arc<std::sync::atomic::AtomicU32>,
}

#[cfg(debug_assertions)]
impl DebugHookMetrics {
    fn new() -> Self {
        Self {
            mouse_callback_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_mouse_message: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            last_mouse_data: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn record_mouse(&self, message: u32, mouse_data: u32) {
        self.mouse_callback_count.fetch_add(1, Ordering::Relaxed);
        self.last_mouse_message.store(message, Ordering::Relaxed);
        self.last_mouse_data.store(mouse_data, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(crate) struct HookThread {
    thread_id: u32,
    join_handle: Option<JoinHandle<()>>,
    pub(crate) metrics: HookThreadMetrics,
}

impl HookThread {
    pub(crate) fn spawn(
        producer: Producer<InputEvent>,
        configuration: InputConfiguration,
    ) -> Result<Self> {
        let metrics = HookThreadMetrics::new();
        let thread_metrics = metrics.clone();
        let (startup_sender, startup_receiver) = bounded::<Result<u32, String>>(1);

        let join_handle = thread::Builder::new()
            .name("gradial-input".to_owned())
            .spawn(move || {
                run_hook_thread(producer, configuration, &thread_metrics, &startup_sender);
            })
            .context("could not spawn the dedicated input-hook thread")?;

        let startup = startup_receiver
            .recv_timeout(Duration::from_secs(3))
            .context("the input-hook thread did not report startup")?;

        match startup {
            Ok(thread_id) => Ok(Self { thread_id, join_handle: Some(join_handle), metrics }),
            Err(message) => {
                let _ = join_handle.join();
                Err(anyhow!(message))
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        if self.thread_id != 0 {
            // SAFETY: the ID belongs to the live Gradial hook thread. WM_QUIT
            // asks its private message loop to stop without touching user input.
            let result =
                unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            if let Err(error) = result {
                log::warn!(
                    target: "input",
                    "component=input event_code=hook_stop_signal_failed result_code=failure \
                     config_revision=0 error={error}"
                );
            }
            self.thread_id = 0;
        }

        if let Some(join_handle) = self.join_handle.take()
            && join_handle.join().is_err()
        {
            log::error!(
                target: "input",
                "component=input event_code=hook_thread_panicked result_code=failure \
                 config_revision=0"
            );
        }
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_hook_thread(
    producer: Producer<InputEvent>,
    configuration: InputConfiguration,
    metrics: &HookThreadMetrics,
    startup_sender: &crossbeam_channel::Sender<Result<u32, String>>,
) {
    CALLBACK_STATE.with(|slot| {
        slot.replace(Some(HookCallbackState::new(producer, configuration, metrics)));
    });

    let thread_id = unsafe { GetCurrentThreadId() };
    let raw_input_window = match RawInputWindow::create() {
        Ok(window) => window,
        Err(error) => {
            CALLBACK_STATE.with(|slot| {
                slot.replace(None);
            });
            let _ = startup_sender.send(Err(error.to_string()));
            return;
        }
    };
    let hooks = match install_hooks() {
        Ok(hooks) => hooks,
        Err(error) => {
            CALLBACK_STATE.with(|slot| {
                slot.replace(None);
            });
            drop(raw_input_window);
            let _ = startup_sender.send(Err(error.to_string()));
            return;
        }
    };

    // SAFETY: a null HWND creates a timer associated with this thread's
    // message queue. No callback is used; WM_TIMER is handled below.
    let timer_id = unsafe { SetTimer(None, HEARTBEAT_TIMER_ID, HEARTBEAT_INTERVAL_MS, None) };
    if timer_id == 0 {
        uninstall_hooks(&hooks);
        CALLBACK_STATE.with(|slot| {
            slot.replace(None);
        });
        let _ = startup_sender.send(Err("could not create the hook heartbeat timer".to_owned()));
        return;
    }

    metrics.heartbeat_qpc.store(query_qpc(), Ordering::Release);
    if startup_sender.send(Ok(thread_id)).is_err() {
        uninstall_hooks(&hooks);
        CALLBACK_STATE.with(|slot| {
            slot.replace(None);
        });
        return;
    }

    log::info!(
        target: "input",
        "component=input event_code=hook_thread_responsive result_code=ok config_revision=0"
    );

    let mut message = MSG::default();
    loop {
        // SAFETY: `message` remains valid for the duration of the call and this
        // dedicated thread owns the message loop.
        let status = unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0;
        if status <= 0 {
            if status < 0 {
                log::error!(
                    target: "input",
                    "component=input event_code=hook_message_loop_failed result_code=failure \
                     config_revision=0"
                );
            }
            break;
        }

        if message.message == WM_TIMER && message.wParam.0 == timer_id {
            metrics.heartbeat_qpc.store(query_qpc(), Ordering::Release);
            continue;
        }

        // SAFETY: the message was populated by GetMessageW on this thread.
        unsafe {
            let _translated = TranslateMessage(&raw const message);
            let _result = DispatchMessageW(&raw const message);
        }
    }

    // SAFETY: this is the same thread and timer identity used by SetTimer.
    if let Err(error) = unsafe { KillTimer(None, timer_id) } {
        log::warn!(
            target: "input",
            "component=input event_code=heartbeat_timer_cleanup_failed result_code=failure \
             config_revision=0 error={error}"
        );
    }

    uninstall_hooks(&hooks);
    CALLBACK_STATE.with(|slot| {
        slot.replace(None);
    });
    drop(raw_input_window);
}

#[derive(Debug)]
struct InstalledHooks {
    mouse: HHOOK,
    keyboard: HHOOK,
}

fn install_hooks() -> Result<InstalledHooks> {
    // SAFETY: low-level hooks with a null module handle and thread ID zero are
    // delivered back to this installing thread, which owns a message loop.
    let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0) }
        .context("could not install the M4 low-level mouse hook")?;

    // SAFETY: same contract as the mouse hook above.
    let keyboard =
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0) } {
            Ok(keyboard) => keyboard,
            Err(error) => {
                // SAFETY: `mouse` was installed successfully in this function.
                let _ = unsafe { UnhookWindowsHookEx(mouse) };
                return Err(error).context("could not install the Escape low-level keyboard hook");
            }
        };

    Ok(InstalledHooks { mouse, keyboard })
}

fn uninstall_hooks(hooks: &InstalledHooks) {
    // SAFETY: both handles were installed by this thread and are consumed once.
    if let Err(error) = unsafe { UnhookWindowsHookEx(hooks.keyboard) } {
        log::warn!(
            target: "input",
            "component=input event_code=keyboard_hook_cleanup_failed result_code=failure \
             config_revision=0 error={error}"
        );
    }
    // SAFETY: same ownership guarantee as the keyboard handle.
    if let Err(error) = unsafe { UnhookWindowsHookEx(hooks.mouse) } {
        log::warn!(
            target: "input",
            "component=input event_code=mouse_hook_cleanup_failed result_code=failure \
             config_revision=0 error={error}"
        );
    }
}

unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < i32::try_from(HC_ACTION).expect("constant fits i32") {
        // SAFETY: required pass-through for negative hook codes.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let callback_started = query_qpc();
    let suppress = CALLBACK_STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return false;
        };

        let Ok(message) = u32::try_from(wparam.0) else {
            return false;
        };
        // SAFETY: Windows supplies an MSLLHOOKSTRUCT pointer for WH_MOUSE_LL.
        let mouse = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        state.handle_mouse(message, mouse, callback_started)
    });
    record_callback_duration(callback_started);

    if suppress {
        LRESULT(1)
    } else {
        // SAFETY: unhandled events must continue through the hook chain.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < i32::try_from(HC_ACTION).expect("constant fits i32") {
        // SAFETY: required pass-through for negative hook codes.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let callback_started = query_qpc();
    let suppress = CALLBACK_STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return false;
        };

        let Ok(message) = u32::try_from(wparam.0) else {
            return false;
        };
        // SAFETY: Windows supplies a KBDLLHOOKSTRUCT pointer for WH_KEYBOARD_LL.
        let keyboard = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        state.handle_keyboard(message, keyboard, callback_started)
    });
    record_callback_duration(callback_started);

    if suppress {
        LRESULT(1)
    } else {
        // SAFETY: unhandled events must continue through the hook chain.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

fn record_callback_duration(started: i64) {
    let duration = query_qpc().saturating_sub(started);
    CALLBACK_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.maximum_callback_qpc.fetch_max(duration, Ordering::Relaxed);
        }
    });
}

pub(super) fn dispatch_raw_delta(dx: i32, dy: i32) {
    let timestamp = query_qpc();
    CALLBACK_STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.handle_raw_delta(dx, dy, timestamp);
        }
    });
}

struct HookCallbackState {
    producer: Producer<InputEvent>,
    queue_overflow: Arc<AtomicBool>,
    maximum_callback_qpc: Arc<AtomicI64>,
    #[cfg(debug_assertions)]
    debug: DebugHookMetrics,
    configuration: InputConfiguration,
    suppression: SuppressionState,
}

impl HookCallbackState {
    fn new(
        producer: Producer<InputEvent>,
        configuration: InputConfiguration,
        metrics: &HookThreadMetrics,
    ) -> Self {
        Self {
            producer,
            queue_overflow: Arc::clone(&metrics.queue_overflow),
            maximum_callback_qpc: Arc::clone(&metrics.maximum_callback_qpc),
            #[cfg(debug_assertions)]
            debug: metrics.debug.clone(),
            configuration,
            suppression: SuppressionState::default(),
        }
    }

    fn handle_mouse(&mut self, message: u32, mouse: &MSLLHOOKSTRUCT, timestamp: i64) -> bool {
        #[cfg(debug_assertions)]
        self.debug.record_mouse(message, mouse.mouseData);

        if mouse.dwExtraInfo == GRADIAL_INJECTED_MARKER {
            return false;
        }

        let point = ScreenPoint::new(mouse.pt.x, mouse.pt.y);
        match message {
            WM_XBUTTONDOWN if high_word(mouse.mouseData) == XBUTTON1 => {
                if self.suppression.geometry.is_none() {
                    let geometry = resolve_invocation_geometry(
                        self.configuration.geometry_mode,
                        point,
                        &self.configuration.monitors,
                        DEFAULT_OVERLAY_HALF_EXTENT_DIP,
                    );
                    self.suppression.geometry = Some(geometry.behavior);
                    self.suppression.cancel_sent = false;
                    self.push(InputEvent::new(
                        timestamp,
                        point,
                        InputEventKind::ActivationDown(geometry),
                    ));
                }
                true
            }
            WM_XBUTTONUP if high_word(mouse.mouseData) == XBUTTON1 => {
                if self.suppression.geometry.is_some() {
                    self.suppression.geometry = None;
                    self.suppression.cancel_sent = false;
                    self.push(InputEvent::new(timestamp, point, InputEventKind::ActivationUp));
                    true
                } else {
                    false
                }
            }
            WM_MOUSEMOVE if self.suppression.geometry.is_some() => {
                if self.suppression.geometry == Some(ResolvedGeometry::EdgeAssist) {
                    true
                } else {
                    self.push(InputEvent::new(timestamp, point, InputEventKind::PointerMoved));
                    false
                }
            }
            WM_LBUTTONDOWN if self.suppression.geometry.is_some() => {
                self.cancel_from_mouse(point, timestamp, SUPPRESSED_LEFT)
            }
            WM_RBUTTONDOWN if self.suppression.geometry.is_some() => {
                self.cancel_from_mouse(point, timestamp, SUPPRESSED_RIGHT)
            }
            WM_MBUTTONDOWN if self.suppression.geometry.is_some() => {
                self.cancel_from_mouse(point, timestamp, SUPPRESSED_MIDDLE)
            }
            WM_XBUTTONDOWN
                if self.suppression.geometry.is_some()
                    && high_word(mouse.mouseData) == XBUTTON2 =>
            {
                self.cancel_from_mouse(point, timestamp, SUPPRESSED_XBUTTON2)
            }
            WM_LBUTTONUP if self.suppression.take_suppressed(SUPPRESSED_LEFT) => true,
            WM_RBUTTONUP if self.suppression.take_suppressed(SUPPRESSED_RIGHT) => true,
            WM_MBUTTONUP if self.suppression.take_suppressed(SUPPRESSED_MIDDLE) => true,
            WM_XBUTTONUP
                if high_word(mouse.mouseData) == XBUTTON2
                    && self.suppression.take_suppressed(SUPPRESSED_XBUTTON2) =>
            {
                true
            }
            _ => false,
        }
    }

    fn handle_raw_delta(&mut self, dx: i32, dy: i32, timestamp: i64) {
        if self.suppression.geometry == Some(ResolvedGeometry::EdgeAssist) {
            self.push(InputEvent::new(
                timestamp,
                ScreenPoint::default(),
                InputEventKind::RawPointerDelta { dx, dy },
            ));
        }
    }

    fn handle_keyboard(
        &mut self,
        message: u32,
        keyboard: &KBDLLHOOKSTRUCT,
        timestamp: i64,
    ) -> bool {
        if keyboard.dwExtraInfo == GRADIAL_INJECTED_MARKER
            || keyboard.vkCode != u32::from(VK_ESCAPE.0)
        {
            return false;
        }

        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN if self.suppression.geometry.is_some() => {
                if !self.suppression.escape_down {
                    self.suppression.escape_down = true;
                    self.send_cancel(ScreenPoint::default(), timestamp, CancelReason::Escape);
                }
                true
            }
            WM_KEYUP | WM_SYSKEYUP if self.suppression.escape_down => {
                self.suppression.escape_down = false;
                true
            }
            _ => false,
        }
    }

    fn cancel_from_mouse(&mut self, point: ScreenPoint, timestamp: i64, button_mask: u8) -> bool {
        self.suppression.suppressed_mouse_buttons |= button_mask;
        self.send_cancel(point, timestamp, CancelReason::OtherMouseButton);
        true
    }

    fn send_cancel(&mut self, point: ScreenPoint, timestamp: i64, reason: CancelReason) {
        if !self.suppression.cancel_sent {
            self.suppression.cancel_sent = true;
            self.push(InputEvent::new(timestamp, point, InputEventKind::Cancel(reason)));
        }
    }

    fn push(&mut self, event: InputEvent) {
        if self.producer.push(event).is_err() {
            self.queue_overflow.store(true, Ordering::Release);
        }
    }
}

#[derive(Debug, Default)]
struct SuppressionState {
    geometry: Option<ResolvedGeometry>,
    cancel_sent: bool,
    escape_down: bool,
    suppressed_mouse_buttons: u8,
}

impl SuppressionState {
    fn take_suppressed(&mut self, mask: u8) -> bool {
        let was_suppressed = self.suppressed_mouse_buttons & mask != 0;
        self.suppressed_mouse_buttons &= !mask;
        was_suppressed
    }
}

fn high_word(value: u32) -> u16 {
    u16::try_from(value >> 16).expect("the high word always fits u16")
}

fn query_qpc() -> i64 {
    let mut timestamp = 0_i64;
    // SAFETY: the pointer refers to a valid local i64 for the duration of the
    // call. QPC is safe and allocation-free on supported Windows versions.
    if unsafe { QueryPerformanceCounter(&raw mut timestamp) }.is_ok() { timestamp } else { 0 }
}

#[cfg(test)]
mod tests {
    use gradial_core::{
        CancelReason, GeometryMode, InputEvent, InputEventKind, InvocationGeometry,
        MonitorGeometry, ResolvedGeometry, ScreenPoint, ScreenRect,
    };
    use rtrb::{Consumer, RingBuffer};
    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::VK_ESCAPE,
            WindowsAndMessaging::{
                KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_XBUTTONDOWN, WM_XBUTTONUP,
            },
        },
    };

    use super::{GRADIAL_INJECTED_MARKER, HookCallbackState, InputConfiguration, XBUTTON1};

    fn callback_state() -> (HookCallbackState, Consumer<InputEvent>) {
        callback_state_for(GeometryMode::Auto)
    }

    fn callback_state_for(
        geometry_mode: GeometryMode,
    ) -> (HookCallbackState, Consumer<InputEvent>) {
        let (producer, consumer) = RingBuffer::new(16);
        let virtual_screen = ScreenRect::new(-2_000, -2_000, 2_000, 2_000);
        let configuration = InputConfiguration {
            geometry_mode,
            monitors: vec![MonitorGeometry {
                bounds: virtual_screen,
                work_area: virtual_screen,
                dpi: 96,
            }],
        };
        (
            HookCallbackState::new(producer, configuration, &super::HookThreadMetrics::new()),
            consumer,
        )
    }

    fn mouse(point: ScreenPoint, xbutton: u16) -> MSLLHOOKSTRUCT {
        MSLLHOOKSTRUCT {
            pt: POINT { x: point.x, y: point.y },
            mouseData: u32::from(xbutton) << 16,
            ..Default::default()
        }
    }

    #[test]
    fn m4_is_suppressed_and_emits_a_bounded_gesture_stream() {
        let (mut state, mut consumer) = callback_state();
        let origin = ScreenPoint::new(-300, 200);
        let m4 = mouse(origin, XBUTTON1);

        assert!(state.handle_mouse(WM_XBUTTONDOWN, &m4, 10));
        assert_eq!(
            consumer.pop(),
            Ok(InputEvent::new(
                10,
                origin,
                InputEventKind::ActivationDown(InvocationGeometry::fixed(origin, 96)),
            ))
        );

        assert!(state.handle_mouse(WM_XBUTTONDOWN, &m4, 11));
        assert!(consumer.pop().is_err(), "auto-repeat must not duplicate activation");

        let moved_point = ScreenPoint::new(-240, 200);
        assert!(!state.handle_mouse(WM_MOUSEMOVE, &mouse(moved_point, 0), 12));
        assert_eq!(
            consumer.pop(),
            Ok(InputEvent::new(12, moved_point, InputEventKind::PointerMoved))
        );

        assert!(state.handle_mouse(WM_XBUTTONUP, &mouse(moved_point, XBUTTON1), 13));
        assert_eq!(
            consumer.pop(),
            Ok(InputEvent::new(13, moved_point, InputEventKind::ActivationUp))
        );
        assert!(!state.handle_mouse(WM_XBUTTONUP, &m4, 14));
    }

    #[test]
    fn edge_assist_suppresses_cursor_motion_and_forwards_raw_deltas() {
        let (mut state, mut consumer) = callback_state_for(GeometryMode::EdgeAssist);
        let origin = ScreenPoint::new(0, 0);
        let m4 = mouse(origin, XBUTTON1);

        assert!(state.handle_mouse(WM_XBUTTONDOWN, &m4, 10));
        assert_eq!(
            consumer.pop(),
            Ok(InputEvent::new(
                10,
                origin,
                InputEventKind::ActivationDown(InvocationGeometry {
                    behavior: ResolvedGeometry::EdgeAssist,
                    overlay_origin_physical_px: origin,
                    monitor_dpi: 96,
                }),
            ))
        );

        let moved_point = ScreenPoint::new(60, 0);
        assert!(state.handle_mouse(WM_MOUSEMOVE, &mouse(moved_point, 0), 11));
        assert!(consumer.pop().is_err(), "absolute cursor motion is not a gesture event");

        state.handle_raw_delta(60, 0, 12);
        assert_eq!(
            consumer.pop(),
            Ok(InputEvent::new(
                12,
                ScreenPoint::default(),
                InputEventKind::RawPointerDelta { dx: 60, dy: 0 },
            ))
        );
    }

    #[test]
    fn gradial_injected_events_are_never_intercepted() {
        let (mut state, mut consumer) = callback_state();
        let injected = MSLLHOOKSTRUCT {
            mouseData: u32::from(XBUTTON1) << 16,
            dwExtraInfo: GRADIAL_INJECTED_MARKER,
            ..Default::default()
        };

        assert!(!state.handle_mouse(WM_XBUTTONDOWN, &injected, 10));
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn escape_cancels_once_and_its_matching_release_is_suppressed() {
        let (mut state, mut consumer) = callback_state();
        let m4 = mouse(ScreenPoint::default(), XBUTTON1);
        assert!(state.handle_mouse(WM_XBUTTONDOWN, &m4, 10));
        consumer.pop().expect("activation event");

        let escape = KBDLLHOOKSTRUCT { vkCode: u32::from(VK_ESCAPE.0), ..Default::default() };
        assert!(state.handle_keyboard(WM_KEYDOWN, &escape, 11));
        assert_eq!(
            consumer.pop(),
            Ok(InputEvent::new(
                11,
                ScreenPoint::default(),
                InputEventKind::Cancel(CancelReason::Escape),
            ))
        );

        assert!(state.handle_keyboard(WM_KEYDOWN, &escape, 12));
        assert!(consumer.pop().is_err(), "key repeat must not duplicate cancellation");
        assert!(state.handle_keyboard(WM_KEYUP, &escape, 13));
        assert!(!state.handle_keyboard(WM_KEYUP, &escape, 14));
    }

    #[test]
    fn another_mouse_button_cancels_and_suppresses_its_release() {
        let (mut state, mut consumer) = callback_state();
        let origin = ScreenPoint::new(50, 60);
        assert!(state.handle_mouse(WM_XBUTTONDOWN, &mouse(origin, XBUTTON1), 10));
        consumer.pop().expect("activation event");

        assert!(state.handle_mouse(WM_LBUTTONDOWN, &mouse(origin, 0), 11));
        assert_eq!(
            consumer.pop(),
            Ok(
                InputEvent::new(11, origin, InputEventKind::Cancel(CancelReason::OtherMouseButton),)
            )
        );
        assert!(state.handle_mouse(WM_LBUTTONUP, &mouse(origin, 0), 12));
        assert!(!state.handle_mouse(WM_LBUTTONUP, &mouse(origin, 0), 13));
    }
}
