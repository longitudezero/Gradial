use std::{ffi::c_void, mem::size_of};

use anyhow::{Context, Result, bail};
use windows::{
    Win32::{
        Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::{
                GetRawInputData, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE,
                RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE,
                RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW,
                UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WNDCLASSW,
            },
        },
    },
    core::w,
};

const RAW_INPUT_CLASS_NAME: windows::core::PCWSTR = w!("GradialRawInputWindow");
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_MOUSE: u16 = 0x02;

#[derive(Debug)]
pub(super) struct RawInputWindow {
    window: HWND,
    instance: HINSTANCE,
}

impl RawInputWindow {
    pub(super) fn create() -> Result<Self> {
        // SAFETY: a null module name resolves the module containing Gradial.
        let module =
            unsafe { GetModuleHandleW(None) }.context("could not resolve Gradial's module")?;
        let instance = HINSTANCE(module.0);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(raw_input_window_proc),
            hInstance: instance,
            lpszClassName: RAW_INPUT_CLASS_NAME,
            ..Default::default()
        };

        // SAFETY: the class descriptor and static class name outlive the call.
        if unsafe { RegisterClassW(&raw const window_class) } == 0 {
            let error = unsafe { GetLastError() };
            bail!("could not register the raw-input window class: Windows error {}", error.0);
        }

        // SAFETY: this creates a message-only window owned by the input thread.
        let window = match unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                RAW_INPUT_CLASS_NAME,
                w!("Gradial Raw Input"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
        } {
            Ok(window) => window,
            Err(error) => {
                // SAFETY: the class was registered by this function and has
                // no windows after CreateWindowExW failed.
                let _ = unsafe { UnregisterClassW(RAW_INPUT_CLASS_NAME, Some(instance)) };
                return Err(error).context("could not create the raw-input message window");
            }
        };

        let device = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        };
        let structure_size =
            u32::try_from(size_of::<RAWINPUTDEVICE>()).context("RAWINPUTDEVICE is too large")?;
        // SAFETY: `window` remains valid until this object is dropped.
        if let Err(error) = unsafe { RegisterRawInputDevices(&[device], structure_size) } {
            // SAFETY: both objects were created by this thread above.
            let _ = unsafe { DestroyWindow(window) };
            let _ = unsafe { UnregisterClassW(RAW_INPUT_CLASS_NAME, Some(instance)) };
            return Err(error).context("could not register the raw mouse input sink");
        }

        Ok(Self { window, instance })
    }
}

impl Drop for RawInputWindow {
    fn drop(&mut self) {
        let remove = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND::default(),
        };
        let structure_size = u32::try_from(size_of::<RAWINPUTDEVICE>()).unwrap_or(u32::MAX);
        // SAFETY: removal is process-wide for this usage pair and requires a
        // null target. The message window and class are then released in order.
        if let Err(error) = unsafe { RegisterRawInputDevices(&[remove], structure_size) } {
            log::warn!(
                target: "input",
                "component=input event_code=raw_input_unregister_failed result_code=failure \
                 config_revision=0 error={error}"
            );
        }
        if let Err(error) = unsafe { DestroyWindow(self.window) } {
            log::warn!(
                target: "input",
                "component=input event_code=raw_input_window_cleanup_failed result_code=failure \
                 config_revision=0 error={error}"
            );
        }
        if let Err(error) = unsafe { UnregisterClassW(RAW_INPUT_CLASS_NAME, Some(self.instance)) } {
            log::warn!(
                target: "input",
                "component=input event_code=raw_input_class_cleanup_failed result_code=failure \
                 config_revision=0 error={error}"
            );
        }
    }
}

unsafe extern "system" fn raw_input_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        read_raw_mouse(lparam);
    }

    // SAFETY: all messages, including WM_INPUT, are forwarded for the default
    // cleanup required by the raw-input contract.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn read_raw_mouse(lparam: LPARAM) {
    let Ok(address) = usize::try_from(lparam.0) else {
        return;
    };
    let raw_handle = HRAWINPUT(std::ptr::with_exposed_provenance_mut(address));
    let mut raw_input = RAWINPUT::default();
    let Ok(mut input_size) = u32::try_from(size_of::<RAWINPUT>()) else {
        return;
    };
    let Ok(header_size) = u32::try_from(size_of::<RAWINPUTHEADER>()) else {
        return;
    };

    // SAFETY: `raw_handle` comes from WM_INPUT and the output buffer is a live,
    // correctly sized RAWINPUT value.
    let bytes_read = unsafe {
        GetRawInputData(
            raw_handle,
            RID_INPUT,
            Some((&raw mut raw_input).cast::<c_void>()),
            &raw mut input_size,
            header_size,
        )
    };
    if bytes_read == u32::MAX
        || bytes_read < header_size
        || raw_input.header.dwType != RIM_TYPEMOUSE.0
    {
        return;
    }

    // SAFETY: the header identified the active union member as a mouse.
    let mouse = unsafe { raw_input.data.mouse };
    if mouse.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0 != 0 || (mouse.lLastX == 0 && mouse.lLastY == 0) {
        return;
    }

    super::hooks::dispatch_raw_delta(mouse.lLastX, mouse.lLastY);
}
