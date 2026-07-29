use std::{
    ffi::OsString,
    mem::size_of,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
};

#[cfg(debug_assertions)]
use std::sync::atomic::AtomicU8;

use anyhow::{Context, Result, bail};
use gradial_core::{
    ActivationBindingId, InvocationContext, MonitorGeometry, PhysicalModifiers, ScreenPoint,
    ScreenRect,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{
        CloseHandle, FILETIME, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SetLastError,
        WIN32_ERROR, WPARAM,
    },
    Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, InvalidateRect,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
    },
    System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    },
    System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::{
        HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            FindWindowW, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW,
            GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE, PBT_APMRESUMEAUTOMATIC,
            PBT_APMRESUMECRITICAL, PBT_APMRESUMESTANDBY, PBT_APMRESUMESUSPEND, PostMessageW,
            RegisterWindowMessageW, SET_WINDOW_POS_FLAGS, SPI_SETWORKAREA, SW_HIDE,
            SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_APP, WM_DISPLAYCHANGE,
            WM_DPICHANGED, WM_MOUSEACTIVATE, WM_NCDESTROY, WM_NCHITTEST, WM_POWERBROADCAST,
            WM_SETTINGCHANGE, WM_WTSSESSION_CHANGE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            WTS_SESSION_UNLOCK,
        },
    },
};
use windows::core::{BOOL, PCWSTR, PWSTR};

const OVERLAY_SUBCLASS_ID: usize = 0x4752_444C;
const WM_GRADIAL_SHOW_OVERLAY: u32 = WM_APP + 0x244;
const WM_GRADIAL_HIDE_OVERLAY: u32 = WM_APP + 0x245;
#[cfg(debug_assertions)]
const WM_GRADIAL_DEBUG_HOOK_UNRESPONSIVE: u32 = WM_APP + 0x247;
#[cfg(debug_assertions)]
const WM_GRADIAL_DEBUG_QUEUE_OVERFLOW: u32 = WM_APP + 0x248;
#[cfg(debug_assertions)]
const WM_GRADIAL_DEBUG_RETRY_INPUT: u32 = WM_APP + 0x249;
static DISPLAY_TOPOLOGY_MONITORING: AtomicBool = AtomicBool::new(false);
static DISPLAY_TOPOLOGY_CHANGED: AtomicBool = AtomicBool::new(false);
static CONTROLLER_WINDOW_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static CONTROLLER_REPAINT_CONTEXT: OnceLock<eframe::egui::Context> = OnceLock::new();
static TASKBAR_CREATED_MESSAGE: AtomicU32 = AtomicU32::new(0);
static TRAY_READD_REQUESTED: AtomicBool = AtomicBool::new(false);
static INPUT_REREGISTRATION_REQUESTED: AtomicBool = AtomicBool::new(false);
#[cfg(debug_assertions)]
static DEBUG_INPUT_FAILURE: AtomicU8 = AtomicU8::new(0);

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DebugInputFailure {
    HookUnresponsive,
    QueueOverflow,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ForegroundWindowIdentity {
    pub(crate) window: usize,
    pub(crate) process_id: u32,
}

pub(crate) fn foreground_window_identity() -> ForegroundWindowIdentity {
    // SAFETY: GetForegroundWindow has no preconditions and the optional
    // process-ID output refers to a live local value.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return ForegroundWindowIdentity::default();
    }

    let mut process_id = 0_u32;
    unsafe {
        GetWindowThreadProcessId(window, Some(&raw mut process_id));
    }
    ForegroundWindowIdentity { window: window.0.addr(), process_id }
}

pub(crate) fn capture_invocation_context(
    point: ScreenPoint,
    physical_modifiers: PhysicalModifiers,
    config_revision: u64,
) -> InvocationContext {
    let foreground = foreground_window_identity();
    let window = HWND(std::ptr::with_exposed_provenance_mut(foreground.window));
    let (process_start_time, executable_path) =
        process_identity(foreground.process_id, config_revision);
    let (monitor_device_name, monitor_rect_physical_px, work_area_physical_px) =
        monitor_identity(point);

    InvocationContext {
        foreground_window: foreground.window,
        foreground_pid: foreground.process_id,
        process_start_time,
        executable_path,
        window_class: window_class(window),
        window_title: window_title(window),
        monitor_device_name,
        monitor_rect_physical_px,
        work_area_physical_px,
        physical_modifiers,
        activation_binding: ActivationBindingId::MouseButton4,
        config_revision,
    }
}

fn process_identity(process_id: u32, config_revision: u64) -> (u64, Option<PathBuf>) {
    if process_id == 0 {
        return (0, None);
    }
    // SAFETY: the requested handle is read-only and `process_id` was returned
    // for the foreground window. A racing process exit is handled as missing
    // target metadata.
    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    else {
        return (0, None);
    };

    let process_start_time = process_start_time(process).unwrap_or_default();
    let executable_path = process_executable_path(process);
    // SAFETY: `process` was opened by this function and is no longer used.
    if let Err(error) = unsafe { CloseHandle(process) } {
        log::warn!(
            target: "interaction",
            "component=interaction event_code=target_process_handle_cleanup_failed \
             result_code=failure config_revision={config_revision} error={error}"
        );
    }
    (process_start_time, executable_path)
}

fn process_start_time(process: windows::Win32::Foundation::HANDLE) -> Option<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the process handle has query rights and all outputs are live.
    unsafe {
        GetProcessTimes(process, &raw mut creation, &raw mut exit, &raw mut kernel, &raw mut user)
            .ok()?;
    }
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn process_executable_path(process: windows::Win32::Foundation::HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len()).ok()?;
    // SAFETY: the process handle has query rights and the output buffer remains
    // writable for the reported capacity.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR::from_raw(buffer.as_mut_ptr()),
            &raw mut length,
        )
        .ok()?;
    }
    let length = usize::try_from(length).ok()?;
    buffer.truncate(length);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

fn window_class(window: HWND) -> String {
    if window.is_invalid() {
        return String::new();
    }
    let mut buffer = [0_u16; 256];
    // SAFETY: `window` is the activation foreground HWND and `buffer` is live.
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or_default()])
}

fn window_title(window: HWND) -> String {
    if window.is_invalid() {
        return String::new();
    }
    // SAFETY: `window` is the activation foreground HWND.
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let Ok(capacity) = usize::try_from(length.saturating_add(1)) else {
        return String::new();
    };
    let mut buffer = vec![0_u16; capacity];
    // SAFETY: `buffer` has room for the measured title and terminator.
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or_default()])
}

fn monitor_identity(point: ScreenPoint) -> (String, ScreenRect, ScreenRect) {
    let win32_point = POINT { x: point.x, y: point.y };
    // SAFETY: the point is a signed virtual-screen coordinate.
    let monitor = unsafe { MonitorFromPoint(win32_point, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return (String::new(), ScreenRect::default(), ScreenRect::default());
    }
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = u32::try_from(size_of::<MONITORINFOEXW>()).unwrap_or(u32::MAX);
    // SAFETY: MONITORINFOEXW begins with MONITORINFO as required by
    // GetMonitorInfoW and remains live for the output.
    let succeeded =
        unsafe { GetMonitorInfoW(monitor, std::ptr::from_mut(&mut info).cast::<MONITORINFO>()) };
    if !succeeded.as_bool() {
        return (String::new(), ScreenRect::default(), ScreenRect::default());
    }
    let device_length =
        info.szDevice.iter().position(|character| *character == 0).unwrap_or(info.szDevice.len());
    (
        String::from_utf16_lossy(&info.szDevice[..device_length]),
        ScreenRect::new(
            info.monitorInfo.rcMonitor.left,
            info.monitorInfo.rcMonitor.top,
            info.monitorInfo.rcMonitor.right,
            info.monitorInfo.rcMonitor.bottom,
        ),
        ScreenRect::new(
            info.monitorInfo.rcWork.left,
            info.monitorInfo.rcWork.top,
            info.monitorInfo.rcWork.right,
            info.monitorInfo.rcWork.bottom,
        ),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlayWakeHandle {
    window_address: usize,
}

pub(crate) fn set_display_topology_monitoring(enabled: bool) {
    if enabled {
        DISPLAY_TOPOLOGY_CHANGED.store(false, Ordering::Release);
        DISPLAY_TOPOLOGY_MONITORING.store(true, Ordering::Release);
    } else {
        DISPLAY_TOPOLOGY_MONITORING.store(false, Ordering::Release);
        DISPLAY_TOPOLOGY_CHANGED.store(false, Ordering::Release);
    }
}

pub(crate) fn take_display_topology_changed() -> bool {
    DISPLAY_TOPOLOGY_CHANGED.swap(false, Ordering::AcqRel)
}

pub(crate) fn controller_window_address() -> usize {
    CONTROLLER_WINDOW_ADDRESS.load(Ordering::Acquire)
}

pub(crate) fn take_tray_readd_requested() -> bool {
    TRAY_READD_REQUESTED.swap(false, Ordering::AcqRel)
}

pub(crate) fn take_input_reregistration_requested() -> bool {
    INPUT_REREGISTRATION_REQUESTED.swap(false, Ordering::AcqRel)
}

#[cfg(debug_assertions)]
pub(crate) fn take_debug_input_failure() -> Option<DebugInputFailure> {
    match DEBUG_INPUT_FAILURE.swap(0, Ordering::AcqRel) {
        1 => Some(DebugInputFailure::HookUnresponsive),
        2 => Some(DebugInputFailure::QueueOverflow),
        _ => None,
    }
}

pub(crate) fn request_controller_repaint() {
    if let Some(context) = CONTROLLER_REPAINT_CONTEXT.get() {
        context.request_repaint_of(eframe::egui::ViewportId::ROOT);
    }
    invalidate_controller();
}

pub(crate) fn request_overlay_wake(handle: OverlayWakeHandle, origin: ScreenPoint) -> Result<()> {
    let window = HWND(std::ptr::with_exposed_provenance_mut(handle.window_address));
    let encoded_x = usize::try_from(u32::from_ne_bytes(origin.x.to_ne_bytes()))
        .context("x coordinate does not fit WPARAM")?;
    let encoded_y = isize::try_from(origin.y).context("y coordinate does not fit LPARAM")?;

    // SAFETY: the handle identifies a live Gradial overlay window. This posts a
    // private message and never synchronously enters the UI thread.
    unsafe {
        PostMessageW(Some(window), WM_GRADIAL_SHOW_OVERLAY, WPARAM(encoded_x), LPARAM(encoded_y))
            .context("could not wake the hidden overlay window")?;
    }
    Ok(())
}

pub(crate) fn request_overlay_hide(handle: OverlayWakeHandle) -> Result<()> {
    let window = HWND(std::ptr::with_exposed_provenance_mut(handle.window_address));
    // SAFETY: the handle identifies a live Gradial overlay window. This posts a
    // private fail-safe hide request without entering the UI thread.
    unsafe {
        PostMessageW(Some(window), WM_GRADIAL_HIDE_OVERLAY, WPARAM(0), LPARAM(0))
            .context("could not request a fail-safe overlay hide")?;
    }
    Ok(())
}

pub(crate) fn request_overlay_repaint(handle: OverlayWakeHandle) -> Result<()> {
    let window = hwnd_from_handle(handle);
    // SAFETY: the handle identifies a live Gradial overlay. Invalidating its
    // client area wakes the visible child even when the root controller is
    // hidden and throttled.
    let invalidated = unsafe { InvalidateRect(Some(window), None, false) };
    if !invalidated.as_bool() {
        bail!("Windows could not invalidate the overlay window");
    }
    Ok(())
}

pub(crate) fn overlay_window_by_title(title: &str) -> Option<OverlayWakeHandle> {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the class name is absent and `wide` is a live, null-terminated
    // title string for the duration of the lookup.
    let window = unsafe { FindWindowW(None, PCWSTR::from_raw(wide.as_ptr())) }.ok()?;
    Some(OverlayWakeHandle { window_address: window.0.addr() })
}

pub(crate) fn configure_overlay_handle(handle: OverlayWakeHandle) -> Result<()> {
    apply_native_window_styles_hwnd(hwnd_from_handle(handle))
}

pub(crate) fn position_hidden_overlay(
    handle: OverlayWakeHandle,
    origin: ScreenPoint,
) -> Result<()> {
    let hwnd = hwnd_from_handle(handle);
    let mut rectangle = RECT::default();

    // SAFETY: the handle identifies a live Gradial viewport and the output
    // points to a local RECT.
    unsafe {
        let _was_visible = ShowWindow(hwnd, SW_HIDE);
        GetWindowRect(hwnd, &raw mut rectangle).context("could not measure the overlay window")?;
    }

    let width = rectangle.right.saturating_sub(rectangle.left);
    let height = rectangle.bottom.saturating_sub(rectangle.top);
    let x = origin.x.saturating_sub(width / 2);
    let y = origin.y.saturating_sub(height / 2);

    // SAFETY: this keeps the live viewport hidden and inactive while moving it
    // onto the display whose DPI it must adopt.
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE)
            .context("could not preposition the hidden overlay")?;
    }
    Ok(())
}

pub(crate) fn overlay_window_dpi(handle: OverlayWakeHandle) -> u32 {
    // SAFETY: the handle identifies a live Gradial viewport.
    unsafe { GetDpiForWindow(hwnd_from_handle(handle)) }
}

pub(crate) fn monitor_work_areas() -> Result<Vec<MonitorGeometry>> {
    let mut monitors = Vec::new();
    let address = isize::try_from((&raw mut monitors).addr())
        .context("monitor collection pointer does not fit LPARAM")?;

    // SAFETY: the callback is synchronous. `address` identifies `monitors`,
    // which remains live and exclusively borrowed until enumeration returns.
    let completed =
        unsafe { EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(address)) };
    if !completed.as_bool() {
        bail!("Windows could not enumerate display work areas");
    }
    if monitors.is_empty() {
        bail!("Windows reported no active display work areas");
    }

    Ok(monitors)
}

pub(crate) fn configure_overlay_window(
    creation_context: &eframe::CreationContext<'_>,
) -> Result<()> {
    let window =
        creation_context.winit_window().context("eframe did not expose its root Windows window")?;
    let hwnd = window_handle(window)?;
    CONTROLLER_WINDOW_ADDRESS.store(hwnd.0.addr(), Ordering::Release);
    let _existing_context = CONTROLLER_REPAINT_CONTEXT.set(creation_context.egui_ctx.clone());
    let taskbar_created = wide("TaskbarCreated");
    // SAFETY: the input is a live, null-terminated system message name.
    let taskbar_message =
        unsafe { RegisterWindowMessageW(PCWSTR::from_raw(taskbar_created.as_ptr())) };
    if taskbar_message == 0 {
        bail!("Windows could not register the TaskbarCreated message");
    }
    TASKBAR_CREATED_MESSAGE.store(taskbar_message, Ordering::Release);
    // SAFETY: the live root window receives notifications only for the current
    // session and unregisters during destruction.
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
            .context("could not register session-unlock notifications")?;
    }

    window.set_cursor_hittest(false).context("could not make the overlay click-through")?;

    apply_native_window_styles(creation_context)
}

pub(crate) fn enforce_overlay_window_styles(frame: &eframe::Frame) -> Result<()> {
    apply_native_window_styles(frame)
}

fn position_and_show_window(hwnd: HWND, origin: ScreenPoint) -> Result<()> {
    let mut rectangle = RECT::default();

    // SAFETY: `hwnd` is a live eframe overlay window and the output pointer
    // refers to a local RECT.
    unsafe {
        GetWindowRect(hwnd, &raw mut rectangle).context("could not measure the overlay window")?;
    }

    let width = rectangle.right.saturating_sub(rectangle.left);
    let height = rectangle.bottom.saturating_sub(rectangle.top);
    let x = origin.x.saturating_sub(width / 2);
    let y = origin.y.saturating_sub(height / 2);
    let flags = SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;

    // SAFETY: the live overlay window is moved in physical screen coordinates,
    // preserving its size and activation state.
    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, flags)
            .context("could not position and show the overlay")?;
        let _was_visible = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    Ok(())
}

fn hwnd_from_handle(handle: OverlayWakeHandle) -> HWND {
    HWND(std::ptr::with_exposed_provenance_mut(handle.window_address))
}

pub(crate) fn hide_overlay(frame: &eframe::Frame) -> Result<()> {
    let hwnd = window_handle(frame)?;
    // SAFETY: `hwnd` is eframe's live root window. Hiding does not activate
    // or destroy it.
    unsafe {
        let _was_visible = ShowWindow(hwnd, SW_HIDE);
    }
    Ok(())
}

fn apply_native_window_styles(window: &impl HasWindowHandle) -> Result<()> {
    let hwnd = window_handle(window)?;
    apply_native_window_styles_hwnd(hwnd)
}

fn apply_native_window_styles_hwnd(hwnd: HWND) -> Result<()> {
    let required_style = isize::try_from((WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0)
        .context("required overlay styles do not fit this Windows target")?;

    // SAFETY: `hwnd` is a live Gradial window owned by this UI thread. The
    // style update, subclass, and position call do not retain Rust references.
    unsafe {
        let current_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired_style = current_style | required_style;

        SetLastError(WIN32_ERROR(0));
        let previous_style = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired_style);
        if previous_style == 0 {
            let error = GetLastError();
            if error != WIN32_ERROR(0) {
                bail!("SetWindowLongPtrW failed with Windows error {}", error.0);
            }
        }

        let confirmed_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if confirmed_style & required_style != required_style {
            bail!(
                "Windows did not retain required overlay styles (requested {required_style:#x}, \
                 found {confirmed_style:#x})"
            );
        }

        if !SetWindowSubclass(hwnd, Some(overlay_subclass_proc), OVERLAY_SUBCLASS_ID, 0).as_bool() {
            let error = GetLastError();
            bail!("SetWindowSubclass failed with Windows error {}", error.0);
        }

        let flags: SET_WINDOW_POS_FLAGS =
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags)
            .context("could not make the overlay topmost without activation")?;
    }

    Ok(())
}

fn window_handle(window: &impl HasWindowHandle) -> Result<HWND> {
    let handle = window.window_handle().context("could not acquire the overlay window handle")?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        bail!("Gradial requires a Win32 window handle");
    };
    Ok(HWND(handle.hwnd.get() as *mut core::ffi::c_void))
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _device_context: HDC,
    _monitor_rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let Ok(address) = usize::try_from(data.0) else {
        return BOOL(0);
    };
    // SAFETY: `monitor_work_areas` supplies the address of its live Vec and
    // EnumDisplayMonitors invokes this callback synchronously.
    let monitors =
        unsafe { &mut *std::ptr::with_exposed_provenance_mut::<Vec<MonitorGeometry>>(address) };

    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    // SAFETY: `monitor` is supplied by Windows and `info` is a valid output.
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        return BOOL(0);
    }

    let mut dpi_x = 96_u32;
    let mut dpi_y = 96_u32;
    // SAFETY: both DPI output pointers remain valid for this call.
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) }
        .is_err()
    {
        dpi_x = 96;
    }

    monitors.push(MonitorGeometry {
        bounds: ScreenRect::new(
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        ),
        work_area: ScreenRect::new(
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        ),
        dpi: dpi_x,
    });
    BOOL(1)
}

unsafe extern "system" fn overlay_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    #[cfg(debug_assertions)]
    if handle_debug_message(hwnd, message) {
        return LRESULT(0);
    }

    let taskbar_created = TASKBAR_CREATED_MESSAGE.load(Ordering::Acquire);
    if taskbar_created != 0 && message == taskbar_created {
        TRAY_READD_REQUESTED.store(true, Ordering::Release);
        request_input_reregistration("explorer_restart");
        // SAFETY: the registered message must continue through the subclass
        // chain after Gradial records it.
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    }

    match message {
        crate::tray::CALLBACK_MESSAGE => {
            if crate::tray::handle_callback(hwnd, lparam.0) {
                request_controller_repaint();
            }
            LRESULT(0)
        }
        WM_GRADIAL_SHOW_OVERLAY => {
            let encoded_x = u32::try_from(wparam.0).unwrap_or_default();
            let origin = ScreenPoint::new(
                i32::from_ne_bytes(encoded_x.to_ne_bytes()),
                i32::try_from(lparam.0).unwrap_or_default(),
            );
            if let Err(error) = position_and_show_window(hwnd, origin) {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=overlay_early_wake_failed \
                     result_code=failure config_revision=0 error={error}"
                );
            }
            LRESULT(0)
        }
        WM_GRADIAL_HIDE_OVERLAY => {
            // SAFETY: `hwnd` is a live Gradial overlay and hiding it does
            // not activate or destroy it.
            unsafe {
                let _was_visible = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            mark_display_topology_changed();
            // SAFETY: unhandled window messages must continue through the
            // Windows subclass chain.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        WM_SETTINGCHANGE
            if wparam.0 == usize::try_from(SPI_SETWORKAREA.0).unwrap_or(usize::MAX) =>
        {
            mark_display_topology_changed();
            // SAFETY: unhandled window messages must continue through the
            // Windows subclass chain.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        WM_POWERBROADCAST
            if matches!(
                u32::try_from(wparam.0).unwrap_or_default(),
                PBT_APMRESUMEAUTOMATIC
                    | PBT_APMRESUMECRITICAL
                    | PBT_APMRESUMESTANDBY
                    | PBT_APMRESUMESUSPEND
            ) =>
        {
            request_input_reregistration("system_resume");
            // SAFETY: the power message must continue through the subclass
            // chain.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        WM_WTSSESSION_CHANGE
            if u32::try_from(wparam.0).unwrap_or_default() == WTS_SESSION_UNLOCK =>
        {
            request_input_reregistration("session_unlock");
            // SAFETY: the session message must continue through the subclass
            // chain.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        WM_MOUSEACTIVATE => LRESULT(isize::try_from(MA_NOACTIVATE).expect("constant fits isize")),
        WM_NCHITTEST => LRESULT(isize::try_from(HTTRANSPARENT).expect("constant fits isize")),
        WM_NCDESTROY => {
            if hwnd.0.addr() == CONTROLLER_WINDOW_ADDRESS.load(Ordering::Acquire) {
                // SAFETY: the root window registered this exact notification
                // scope during construction.
                if let Err(error) = unsafe { WTSUnRegisterSessionNotification(hwnd) } {
                    log::warn!(
                        target: "input",
                        "component=input event_code=session_notification_cleanup_failed \
                         result_code=failure config_revision=0 error={error}"
                    );
                }
                CONTROLLER_WINDOW_ADDRESS.store(0, Ordering::Release);
            }
            // SAFETY: this removes the exact callback installed on this same
            // UI thread, then forwards destruction to the next procedure.
            let _removed = unsafe {
                RemoveWindowSubclass(hwnd, Some(overlay_subclass_proc), OVERLAY_SUBCLASS_ID)
            };
            // SAFETY: forwarding unhandled window messages is required by the
            // Windows subclass contract.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        _ => {
            // SAFETY: forwarding unhandled window messages is required by the
            // Windows subclass contract.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
    }
}

#[cfg(debug_assertions)]
fn handle_debug_message(hwnd: HWND, message: u32) -> bool {
    if hwnd.0.addr() != CONTROLLER_WINDOW_ADDRESS.load(Ordering::Acquire) {
        return false;
    }
    match message {
        WM_GRADIAL_DEBUG_HOOK_UNRESPONSIVE => DEBUG_INPUT_FAILURE.store(1, Ordering::Release),
        WM_GRADIAL_DEBUG_QUEUE_OVERFLOW => DEBUG_INPUT_FAILURE.store(2, Ordering::Release),
        WM_GRADIAL_DEBUG_RETRY_INPUT => crate::tray::request_debug_retry(),
        _ => return false,
    }
    request_controller_repaint();
    true
}

fn mark_display_topology_changed() {
    if DISPLAY_TOPOLOGY_MONITORING.load(Ordering::Acquire) {
        let already_changed = DISPLAY_TOPOLOGY_CHANGED.swap(true, Ordering::AcqRel);
        if !already_changed {
            log::info!(
                target: "renderer",
                "component=renderer event_code=display_topology_signal_observed result_code=ok \
                 config_revision=0"
            );
        }
        request_controller_repaint();
    }
}

fn request_input_reregistration(source: &str) {
    let already_requested = INPUT_REREGISTRATION_REQUESTED.swap(true, Ordering::AcqRel);
    if !already_requested {
        log::info!(
            target: "input",
            "component=input event_code=input_reregistration_requested result_code=ok \
             config_revision=0 source={source}"
        );
    }
    request_controller_repaint();
}

fn invalidate_controller() {
    let controller_address = CONTROLLER_WINDOW_ADDRESS.load(Ordering::Acquire);
    if controller_address == 0 {
        return;
    }
    let controller =
        HWND(std::ptr::with_exposed_provenance_mut::<core::ffi::c_void>(controller_address));
    // SAFETY: the address belongs to Gradial's live root viewport. Invalidating
    // it wakes winit's hidden-window redraw path.
    unsafe {
        let _invalidated = InvalidateRect(Some(controller), None, false);
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
