use anyhow::{Context, Result, bail};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{GetLastError, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE,
            SET_WINDOW_POS_FLAGS, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_MOUSEACTIVATE,
            WM_NCDESTROY, WM_NCHITTEST, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        },
    },
};

const OVERLAY_SUBCLASS_ID: usize = 0x4752_444C;

pub(crate) fn configure_overlay_window(
    creation_context: &eframe::CreationContext<'_>,
) -> Result<()> {
    let window =
        creation_context.winit_window().context("eframe did not expose its root Windows window")?;

    window.set_cursor_hittest(false).context("could not make the overlay click-through")?;

    apply_native_window_styles(creation_context)
}

pub(crate) fn enforce_overlay_window_styles(frame: &eframe::Frame) -> Result<()> {
    apply_native_window_styles(frame)
}

fn apply_native_window_styles(window: &impl HasWindowHandle) -> Result<()> {
    let handle = window.window_handle().context("could not acquire the overlay window handle")?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        bail!("Gradial requires a Win32 window handle");
    };
    let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
    let required_style = isize::try_from((WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0)
        .context("required overlay styles do not fit this Windows target")?;

    // SAFETY: `hwnd` is the live root window owned by this UI thread. The
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

        let _was_visible = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    Ok(())
}

unsafe extern "system" fn overlay_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(isize::try_from(MA_NOACTIVATE).expect("constant fits isize")),
        WM_NCHITTEST => LRESULT(isize::try_from(HTTRANSPARENT).expect("constant fits isize")),
        WM_NCDESTROY => {
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
